// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The command line, the app's own binary run with arguments: `awake on 1h`, `awake off`,
//! `awake status`, holds for scripts and agents' hooks (`awake hold claude --for 1h`,
//! `awake release claude`), and `awake -- make build`, awake while the command runs. Each command
//! asks the running app over a Unix socket, one line there and a reply back ([`Request`]), and
//! opens the app first when it would keep the Mac awake. A command's hold lasts as long as its
//! connection: the app lets go when `awake --` ends, however it ends.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nimbo::Proxy;

use crate::INFO;
use crate::power::Assertion;

const USAGE: &str = "\
Keeps the Mac awake through the Awake menu bar app.

Usage:
  awake on [LENGTH]               until turned off, or for LENGTH: 90 (minutes), 45m, 2h, 1h30m
  awake off                       turn off, holds too
  awake status                    what keeps the Mac awake
  awake hold NAME [--for LENGTH]  until `awake release NAME`, or for LENGTH
  awake release NAME              end the hold NAME
  awake -- COMMAND [ARGS...]      while COMMAND runs; exits with its status
";

/// The longest a session or a hold lasts.
const MAX: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// How long a command waits for the app it opened to listen, and for a reply.
const LAUNCH: Duration = Duration::from_secs(5);

/// What a command asks the app, as a line on the socket: lengths in whole seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
  /// `on [secs]`.
  On(Option<Duration>),
  Off,
  Status,
  /// `hold NAME [secs]`: again renews it.
  Hold(String, Option<Duration>),
  /// `release NAME`: nothing when there is no such hold.
  Release(String),
  /// `run NAME`: held while the connection is open.
  Run(String),
}

impl Request {
  fn line(&self) -> String {
    let secs = |length: &Option<Duration>| length.map(|l| format!(" {}", l.as_secs()));
    match self {
      Request::On(length) => format!("on{}", secs(length).unwrap_or_default()),
      Request::Off => "off".into(),
      Request::Status => "status".into(),
      Request::Hold(name, length) => format!("hold {name}{}", secs(length).unwrap_or_default()),
      Request::Release(name) => format!("release {name}"),
      Request::Run(name) => format!("run {name}"),
    }
  }

  fn parse(line: &str) -> Result<Self, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let secs = |s: &str| {
      let length = s.parse().map(Duration::from_secs).map_err(|_| format!("bad length {s}"))?;
      if length.is_zero() || length > MAX { Err(format!("bad length {s}")) } else { Ok(length) }
    };

    Ok(match words[..] {
      ["on"] => Request::On(None),
      ["on", s] => Request::On(Some(secs(s)?)),
      ["off"] => Request::Off,
      ["status"] => Request::Status,
      ["hold", name] => Request::Hold(name.into(), None),
      ["hold", name, s] => Request::Hold(name.into(), Some(secs(s)?)),
      ["release", name] => Request::Release(name.into()),
      ["run", name] => Request::Run(name.into()),
      _ => return Err(format!("unknown request {line:?}")),
    })
  }

  /// Keeps the Mac awake, so the app opens for it when it isn't running.
  pub fn wakes(&self) -> bool {
    matches!(self, Request::On(_) | Request::Hold(..) | Request::Run(_))
  }
}

/// What the arguments ask for.
#[derive(Debug, PartialEq, Eq)]
enum Cmd {
  Help,
  Ask(Request),
  /// `awake -- COMMAND [ARGS...]`, as given: Unix arguments needn't be UTF-8.
  Exec(Vec<OsString>),
}

fn parse(args: &[OsString]) -> Result<Cmd, String> {
  if let [first, command @ ..] = args
    && first == "--"
    && !command.is_empty()
  {
    return Ok(Cmd::Exec(command.to_vec()));
  }

  let args: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
  let args = args.ok_or("an argument that isn't UTF-8")?;
  let ask = |r| Ok(Cmd::Ask(r));
  match args[..] {
    ["help" | "-h" | "--help"] => Ok(Cmd::Help),
    ["on"] => ask(Request::On(None)),
    ["on", l] => ask(Request::On(Some(length(l)?))),
    ["off"] => ask(Request::Off),
    ["status"] => ask(Request::Status),
    ["hold", n] => ask(Request::Hold(name(n)?, None)),
    ["hold", n, "--for", l] => ask(Request::Hold(name(n)?, Some(length(l)?))),
    ["release", n] => ask(Request::Release(name(n)?)),
    ["--"] => Err("no command after --".into()),
    _ => Err(format!("unknown command: awake {}", args.join(" "))),
  }
}

/// A hold's name: one word, as the socket's line takes it.
fn name(s: &str) -> Result<String, String> {
  if s.is_empty() || s.contains(char::is_whitespace) || s.starts_with('-') {
    return Err(format!("bad name {s:?}: one word"));
  }

  Ok(s.into())
}

/// `90` (minutes), `45s`, `15m`, `2h`, `1h30m`: up to [`MAX`].
fn length(s: &str) -> Result<Duration, String> {
  let bad = || format!("bad length {s:?}: 90 (minutes), 45m, 2h, 1h30m");
  if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
    let minutes: u64 = s.parse().map_err(|_| bad())?;
    return check(Duration::from_secs(minutes.saturating_mul(60))).ok_or_else(bad);
  }

  let (mut total, mut n, mut digits) = (0u64, 0u64, false);
  for c in s.chars() {
    let unit = match c {
      '0'..='9' => {
        n = n.saturating_mul(10).saturating_add(u64::from(c as u8 - b'0'));
        digits = true;
        continue;
      }
      'h' => 3600,
      'm' => 60,
      's' => 1,
      _ => return Err(bad()),
    };

    if !digits {
      return Err(bad());
    }

    total = total.saturating_add(n.saturating_mul(unit));
    (n, digits) = (0, false);
  }

  if digits {
    return Err(bad());
  }

  check(Duration::from_secs(total)).ok_or_else(bad)
}

fn check(length: Duration) -> Option<Duration> {
  (!length.is_zero() && length <= MAX).then_some(length)
}

/// The app's socket: one per app, Awake Dev's its own.
fn socket() -> PathBuf {
  let home = std::env::var_os("HOME").unwrap_or_default();
  Path::new(&home).join("Library/Application Support").join(INFO.id()).join("cli.sock")
}

/// The app answers at its socket: the binary run bare beside it is the command line's help.
pub fn running() -> bool {
  UnixStream::connect(socket()).is_ok()
}

// MARK: Command

/// Runs the command line's `args` (not empty): its exit status.
pub fn main(args: &[OsString]) -> i32 {
  let cmd = match parse(args) {
    Ok(cmd) => cmd,
    Err(e) => {
      eprintln!("awake: {e}\n\n{USAGE}");
      return 2;
    }
  };

  match cmd {
    Cmd::Help => {
      print!("{USAGE}");
      0
    }
    Cmd::Ask(request) => ask(request),
    Cmd::Exec(argv) => exec(&argv),
  }
}

fn ask(request: Request) -> i32 {
  let mut stream = match connect(request.wakes()) {
    Ok(stream) => stream,
    Err(_) if request == Request::Status => {
      println!("Off: {} isn't running", INFO.name());
      return 0;
    }
    Err(_) if matches!(request, Request::Off | Request::Release(_)) => return 0,
    Err(e) => {
      eprintln!("awake: {e}");
      return 1;
    }
  };

  let reply = match send(&mut stream, &request) {
    Ok(reply) => reply,
    Err(e) => {
      eprintln!("awake: {e}");
      return 1;
    }
  };

  if let Some(e) = reply.strip_prefix("error: ") {
    eprintln!("awake: {}", e.trim_end());
    return 1;
  }

  if request == Request::Status {
    print!("{reply}");
  }

  0
}

/// Runs `argv` held by the app, named by its program; held by this process when the app can't be
/// reached, and as the rules say when it says no.
fn exec(argv: &[OsString]) -> i32 {
  let program = Path::new(&argv[0]).file_name().map(|n| n.to_string_lossy().into_owned());
  let name = program.filter(|n| name(n).is_ok()).unwrap_or_else(|| "command".into());
  // Kept open while the command runs: closed, the app lets go.
  let mut held = connect(true);
  let sent = match held.as_mut() {
    Ok(stream) => send(stream, &Request::Run(name)).map_err(|e| e.to_string()),
    Err(e) => Err(e.clone()),
  };

  let _own = match sent {
    Ok(reply) => {
      if let Some(e) = reply.strip_prefix("error: ") {
        eprintln!("awake: {}", e.trim_end());
      }

      None
    }
    Err(e) => {
      eprintln!("awake: {e}: this process keeps the Mac awake");
      Assertion::new(false)
    }
  };

  run(argv)
}

/// Runs `argv`: its exit status, 128 and the signal that ended it, or the shell's 126 when it can't
/// run and 127 when it isn't there.
fn run(argv: &[OsString]) -> i32 {
  match Command::new(&argv[0]).args(&argv[1..]).status() {
    Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
    Err(e) => {
      eprintln!("awake: {}: {e}", argv[0].to_string_lossy());
      if e.kind() == io::ErrorKind::NotFound { 127 } else { 126 }
    }
  }
}

/// The app's socket; with `open`, the app opened first when it doesn't answer. Or why not.
fn connect(open: bool) -> Result<UnixStream, String> {
  let path = socket();
  if let Ok(stream) = UnixStream::connect(&path) {
    return Ok(stream);
  }

  if !open {
    return Err(format!("{} isn't running", INFO.name()));
  }

  let opened = Command::new("open").args(["-g", "-b", INFO.id()]).status();
  if !opened.is_ok_and(|s| s.success()) {
    return Err(format!("{} isn't running and didn't open", INFO.name()));
  }

  let start = Instant::now();
  while start.elapsed() < LAUNCH {
    thread::sleep(Duration::from_millis(100));
    if let Ok(stream) = UnixStream::connect(&path) {
      return Ok(stream);
    }
  }

  Err(format!("{} didn't answer", INFO.name()))
}

/// Sends `request`'s line, gives the reply: its lines up to an empty one, which an app that
/// answers in time sends.
fn send(stream: &mut UnixStream, request: &Request) -> io::Result<String> {
  let late = |e: io::Error| match e.kind() {
    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
      io::Error::new(e.kind(), format!("{} didn't answer", INFO.name()))
    }
    _ => e,
  };

  stream.set_read_timeout(Some(LAUNCH))?;
  stream.set_write_timeout(Some(LAUNCH))?;
  stream.write_all(format!("{}\n", request.line()).as_bytes()).map_err(late)?;
  let mut reply = String::new();
  for line in BufReader::new(&*stream).lines() {
    let line = line.map_err(late)?;
    if line.is_empty() {
      return Ok(reply);
    }

    reply += &line;
    reply.push('\n');
  }

  Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("{} gave no reply", INFO.name())))
}

// MARK: Server

/// What the app does with a request on the main thread: the reply, its lines.
pub type Handler = fn(Request, u64) -> String;

/// Serves the app's socket on threads of its own: each connection's request goes to `on` on the
/// main thread, with the connection's number; a [`Request::Run`]'s connection, once closed, to
/// `closed`. Not when another app of this id answers there (a second binary run bare).
pub fn listen(proxy: Proxy, on: Handler, closed: fn(u64)) {
  let path = socket();
  if UnixStream::connect(&path).is_ok() {
    tracing::warn!("cli: another {} answers at {}", INFO.name(), path.display());
    return;
  }

  if let Some(dir) = path.parent() {
    _ = std::fs::create_dir_all(dir);
  }

  // A socket left by an app that ended.
  _ = std::fs::remove_file(&path);
  let listener = match UnixListener::bind(&path) {
    Ok(listener) => listener,
    Err(e) => {
      tracing::warn!("cli: {}: {e}", path.display());
      return;
    }
  };

  thread::spawn(move || {
    for (conn, stream) in (1..).zip(listener.incoming()) {
      let Ok(stream) = stream else { continue };
      let proxy = proxy.clone();
      thread::spawn(move || serve(&stream, conn, &proxy, on, closed));
    }
  });
}

fn serve(stream: &UnixStream, conn: u64, proxy: &Proxy, on: Handler, closed: fn(u64)) {
  let mut reader = BufReader::new(stream);
  let mut line = String::new();
  if reader.read_line(&mut line).is_err() {
    return;
  }

  let request = Request::parse(line.trim_end());
  let run = matches!(request, Ok(Request::Run(_)));
  let reply = match request {
    Ok(request) => {
      let (tx, rx) = mpsc::channel();
      proxy.post(move |_| _ = tx.send(on(request, conn)));
      rx.recv().unwrap_or_else(|_| "error: the app is quitting".into())
    }
    Err(e) => format!("error: {e}"),
  };

  let mut writer = stream;
  _ = writer.write_all(format!("{}\n\n", reply.trim_end()).as_bytes());
  if run {
    // The command runs until the client closes the connection, or ends.
    _ = io::copy(&mut reader, &mut io::sink());
    proxy.post(move |_| closed(conn));
  }
}

#[cfg(test)]
mod tests {
  use std::os::unix::ffi::OsStringExt;

  use super::*;

  fn args(s: &str) -> Vec<OsString> {
    s.split_whitespace().map(OsString::from).collect()
  }

  fn mins(m: u64) -> Duration {
    Duration::from_secs(m * 60)
  }

  #[test]
  fn lengths_read_as_people_write_them() {
    let good = [
      ("90", mins(90)),
      ("45m", mins(45)),
      ("2h", mins(120)),
      ("1h30m", mins(90)),
      ("30s", Duration::from_secs(30)),
      ("720h", MAX),
    ];
    for (s, want) in good {
      assert_eq!(length(s), Ok(want), "{s}");
    }

    for s in ["", "0", "0m", "h", "1x", "1h30", "-5m", "721h", "99999999999999999999h", "1.5h"] {
      assert!(length(s).is_err(), "{s}");
    }
  }

  #[test]
  fn arguments_read_as_the_usage_says() {
    let ask = |r| Ok(Cmd::Ask(r));
    assert_eq!(parse(&args("on")), ask(Request::On(None)));
    assert_eq!(parse(&args("on 1h")), ask(Request::On(Some(mins(60)))));
    assert_eq!(parse(&args("off")), ask(Request::Off));
    assert_eq!(parse(&args("status")), ask(Request::Status));
    assert_eq!(parse(&args("hold claude")), ask(Request::Hold("claude".into(), None)));

    let held = ask(Request::Hold("claude".into(), Some(mins(10))));
    assert_eq!(parse(&args("hold claude --for 10m")), held);
    assert_eq!(parse(&args("release claude")), ask(Request::Release("claude".into())));
    assert_eq!(parse(&args("-- make -j8")), Ok(Cmd::Exec(args("make -j8"))));
    assert_eq!(parse(&args("--help")), Ok(Cmd::Help));

    let bytes = OsString::from_vec(vec![0xff]);
    let exec = parse(&[OsString::from("--"), "printf".into(), bytes.clone()]);
    assert_eq!(exec, Ok(Cmd::Exec(vec!["printf".into(), bytes.clone()])), "any bytes");
    assert!(parse(&[OsString::from("hold"), bytes]).is_err());

    for bad in ["", "on 1x", "on 1h 2h", "hold", "hold --for 1h", "hold a --for", "--", "nap"] {
      assert!(parse(&args(bad)).is_err(), "{bad:?}");
    }
  }

  #[test]
  fn a_command_that_cant_run_exits_as_a_shell_says() {
    assert_eq!(run(&args("/nonexistent/awake-test")), 127, "not there");
    assert_eq!(run(&args("/etc/hosts")), 126, "not a program");
    assert_eq!(run(&["sh".into(), "-c".into(), "exit 3".into()]), 3);
  }

  #[test]
  fn a_request_reads_back_from_its_line() {
    let requests = [
      Request::On(None),
      Request::On(Some(mins(90))),
      Request::Off,
      Request::Status,
      Request::Hold("claude".into(), None),
      Request::Hold("claude".into(), Some(Duration::from_secs(30))),
      Request::Release("claude".into()),
      Request::Run("make".into()),
    ];
    for r in requests {
      assert_eq!(Request::parse(&r.line()), Ok(r.clone()), "{}", r.line());
    }

    for bad in ["", "on 0", "on x", "on 99999999999", "hold", "nap", "release a b"] {
      assert!(Request::parse(bad).is_err(), "{bad:?}");
    }
  }
}
