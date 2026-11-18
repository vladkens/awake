// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! Awake: keeps the Mac awake from the menu bar, as KeepingYouAwake does. A click on the cup opens
//! the menu: on until turned off or for one of [`LENGTHS`]. The settings window (Settings… in the
//! menu, or the app opened again) has the rest: launch at login, on at launch, the display left to
//! sleep, off when the screen locks, the time left beside the cup, and the battery's rules, which a
//! session turned on by hand outlives. The status item, the menu and the window are nimbo's; what
//! nimbo lacks is AppKit's, in `appkit.rs`. Updates come in place from GitHub's releases
//! (`updater.rs`). Run with arguments, the binary is the command line (`cli.rs`), which asks the
//! running app: on, off, the status, and holds by name or for a command, which keep the Mac awake
//! beside the session.
//!
//!   make run

mod appkit;
mod cli;
mod power;
mod settings;
mod updater;
mod window;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, SystemTime};

use cli::Request;
use nimbo::tray::{Item, Menu, Tray};
use nimbo::{AppInfo, Main, Presence, Proxy, Shortcut, Window, autostart};
use power::{Assertion, Battery, Change};
use settings::Settings;
use tracing_subscriber::EnvFilter;
use updater::Updater;
use window::{Form, FormEvent, Shown};

/// This build's, which the update feed is compared to.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The app this build is: [RELEASE], or [DEV] in a debug build.
const INFO: AppInfo = if cfg!(debug_assertions) { DEV } else { RELEASE };
/// Awake, as released and installed: its id is in every install (settings, login item).
const RELEASE: AppInfo =
  AppInfo { id: "me.vladkens.awake", name: "Awake", presence: Presence::Accessory };
/// Awake Dev, `make run`'s: an app of its own beside an installed Awake, with its own settings,
/// login item and instance, all by its id.
const DEV: AppInfo =
  AppInfo { id: "me.vladkens.awake-dev", name: "Awake Dev", presence: Presence::Accessory };
const HOUR: Duration = minutes(60);
/// The timed sessions of the Keep Awake For submenu.
const LENGTHS: [Duration; 6] =
  [minutes(15), minutes(30), HOUR, minutes(2 * 60), minutes(4 * 60), minutes(8 * 60)];

/// What the menu's items do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
  /// The status line: nothing.
  Status,
  /// Keep Awake while off, Turn Off while on: the item's title changes in the open menu.
  Toggle,
  /// One of [`LENGTHS`], again from its start when it runs.
  For(usize),
  /// Download the update on offer and relaunch as it.
  Update,
  Settings,
  Quit,
}

/// Awake now.
struct Session {
  _assertion: Assertion,
  /// How long it was started for: the submenu's checkmark.
  length: Option<Duration>,
  /// When it ends by itself.
  until: Option<SystemTime>,
  /// Turned on by hand while a rule said no (why): the rules wait until that clears.
  forced: Option<String>,
}

/// A hold from the command line: `awake hold`'s by its name and 0, `awake --`'s by its command's
/// name and connection, while the command runs; with when it ends by itself.
type Holds = BTreeMap<(String, u64), Option<SystemTime>>;

struct State {
  tray: Tray<Action>,
  proxy: Proxy,
  /// While open: opened when asked, dropped when closed.
  window: Option<Window<Form>>,
  settings: Settings,
  session: Option<Session>,
  holds: Holds,
  /// The holds' assertion, while there are any and no rule says no.
  held: Option<Assertion>,
  /// The holds' timer: one that fires for an earlier end finds another number.
  hold_timer: u64,
  /// Why the app last turned off by itself (not the user), until it is on again.
  stopped: Option<String>,
  /// The session's timer: one that fires for an earlier session finds another number.
  timer: u64,
  /// The menu is open: its countdown ticks each second.
  menu_open: bool,
  /// The countdown's tick: one armed before the last [`State::show`] finds another number.
  tick: u64,
  /// The icon's symbol and the time beside it: set again only when they change.
  icon: &'static str,
  title: String,
  updater: Updater,
  /// The update's item in the menu as last set: an open menu keeps it, or its absence.
  update_item: Option<Item<Action>>,
  _watch: Option<power::Watch>,
}

thread_local! {
  static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// `f` with the app's state; nothing when the state is busy further up the stack.
fn with_state(f: impl FnOnce(&mut State)) {
  STATE.with(|cell| match cell.try_borrow_mut() {
    Ok(mut state) => {
      if let Some(state) = state.as_mut() {
        f(state);
      }
    }
    Err(_) => tracing::warn!("awake: state busy, event dropped"),
  });
}

/// A wait of `length` aimed early: dispatch_after fires up to a tenth late, 6 s on the minute, so
/// a long wait aims that much early and what it arms checks the clock and waits for the rest.
fn early(length: Duration) -> Duration {
  if length > Duration::from_secs(1) { length * 10 / 11 } else { length }
}

/// The menu while on or off; `length` is the timed session's, `update` the updater's item.
fn menu(
  status: &str,
  on: bool,
  length: Option<Duration>,
  update: Option<Item<Action>>,
) -> Menu<Action> {
  let lengths = LENGTHS.iter().enumerate();
  let lengths = lengths.map(|(i, &l)| Item::toggle(Action::For(i), &title(l), length == Some(l)));
  let mut items = vec![
    Item::full(Action::Status, status).enabled(false),
    Item::Divider,
    Item::button(Action::Toggle, toggle(on)),
    Item::submenu("Keep Awake For", Menu::new(lengths.collect())),
    Item::Divider,
  ];
  items.extend(update);
  items.push(Item::button(Action::Settings, "Settings…").key(Shortcut::char(',').cmd()));
  let quit = Item::button(Action::Quit, &format!("Quit {}", INFO.name));
  items.push(quit.key(Shortcut::char('q').cmd()));
  Menu::new(items)
}

/// The menu's item for an update on offer, on its way, or failed on its way.
fn update_item(state: Option<&updater::State>) -> Option<Item<Action>> {
  use updater::{State, shown};
  match state? {
    State::Available(r) => {
      Some(Item::button(Action::Update, &format!("Update to {}", shown(&r.version))))
    }
    State::Downloading(r, done) => {
      let title = format!("Downloading {}… {done}%", shown(&r.version));
      Some(Item::button(Action::Update, &title).enabled(false))
    }
    State::Failed(Some(r), _) => {
      Some(Item::button(Action::Update, &format!("Retry Update to {}", shown(&r.version))))
    }
    State::Idle
    | State::Checking
    | State::UpToDate
    | State::Failed(None, _)
    | State::Relaunched(_) => None,
  }
}

const fn minutes(m: u64) -> Duration {
  Duration::from_secs(m * 60)
}

fn toggle(on: bool) -> &'static str {
  if on { "Turn Off" } else { "Keep Awake Indefinitely" }
}

/// A length as the submenu names it: `15 Minutes`, `1 Hour`, `8 Hours`.
fn title(length: Duration) -> String {
  match (length.as_secs() / 3600, length.as_secs() / 60) {
    (0, m) => format!("{m} Minutes"),
    (1, _) => "1 Hour".into(),
    (h, _) => format!("{h} Hours"),
  }
}

// MARK: State

impl State {
  fn on_tray(&mut self, main: &mut Main, action: Action) {
    match action {
      Action::Status => {}
      Action::Settings => self.open_settings(main),
      Action::Quit => main.quit(),
      Action::Toggle if self.session.is_some() || self.held.is_some() => self.off(),
      Action::Toggle => self.start(None, true),
      Action::For(i) => self.start(Some(LENGTHS[i]), true),
      Action::Update => self.install(),
    }
  }

  /// The menu opens or closes: built anew, as the time left, the login item's state and the
  /// battery change while it is closed, and the countdown's pace changes.
  fn on_menu(&mut self, open: bool) {
    self.menu_open = open;
    if open {
      self.check(false);
    }

    self.show();
  }

  fn on_form(&mut self, event: FormEvent) {
    match event {
      FormEvent::Login(on) => match autostart::set(&INFO, on) {
        Ok(autostart::State::RequiresApproval) => autostart::open_settings(),
        Ok(_) => {}
        Err(e) => tracing::warn!("launch at login: {e}"),
      },
      FormEvent::Check => self.check(true),
      FormEvent::Install => self.install(),
      FormEvent::Changed(settings) => {
        let display = settings.display_sleeps != self.settings.display_sleeps;
        self.settings = settings;
        // The assertion of the other kind, for the same session.
        if display && let Some(session) = self.session.take() {
          match Assertion::new(!settings.display_sleeps) {
            Some(assertion) => self.session = Some(Session { _assertion: assertion, ..session }),
            None => self.stop(Some("the system refused".into())),
          }
        }

        if display {
          self.held = None;
        }

        self.settings.save();
        self.enforce();
        self.hold();
      }
    }

    self.show();
  }

  fn on_power(&mut self, change: Change) {
    match change {
      Change::Battery | Change::LowPowerMode => {
        self.enforce();
        self.hold();
      }
      // The queue's clock stopped while the Mac slept: the ends are timed again.
      Change::Woke => {
        self.timer += 1;
        self.expire();
        self.check(false);
        self.hold();
      }
      Change::Locked if self.settings.off_on_lock && self.session.is_some() => {
        self.stop(Some("screen locked".into()));
      }
      Change::Locked => {}
    }

    self.show();
  }

  fn open_settings(&mut self, main: &mut Main) {
    if self.window.is_none() {
      let on: fn(FormEvent) = |event| with_state(|state| state.on_form(event));
      let (cfg, form) = Form::new(self.shown(), self.proxy.clone(), on);
      let window = main.open(&cfg, form);
      window.on_close(|_| with_state(|state| state.window = None));
      self.window = Some(window);
    }

    self.show();
    if let Some(window) = &self.window {
      window.front();
    }
  }

  /// A session of `length`, or until turned off. By hand it starts whatever the rules say, and they
  /// wait until what they object to clears; at launch they decide.
  fn start(&mut self, length: Option<Duration>, by_hand: bool) {
    let why = self.why_not();
    if why.is_some() && !by_hand {
      self.stop(why);
      return;
    }

    let Some(assertion) = Assertion::new(!self.settings.display_sleeps) else {
      self.stop(Some("the system refused".into()));
      return;
    };

    self.timer += 1;
    let until = length.map(|length| SystemTime::now() + length);
    self.session = Some(Session { _assertion: assertion, length, until, forced: why });
    self.stopped = None;

    if let Some(length) = length {
      self.wait(length);
    }

    self.show();
  }

  /// Turn Off, `awake off`: the session and the holds.
  fn off(&mut self) {
    self.holds.clear();
    self.hold();
    self.stop(None);
  }

  /// What the command line asks, `conn` its connection: the status after it, an error when the
  /// system refused to keep the Mac awake.
  fn on_request(&mut self, request: Request, conn: u64) -> String {
    let wakes = request.wakes();
    let by_hand = matches!(request, Request::On(_));
    match request {
      Request::On(length) => self.start(length, true),
      Request::Off => self.off(),
      Request::Status => {}
      Request::Hold(name, length) => {
        let until = length.map(|length| SystemTime::now() + length);
        self.holds.insert((name, 0), until);
      }
      Request::Release(name) => _ = self.holds.remove(&(name, 0)),
      Request::Run(name) => _ = self.holds.insert((name, conn), None),
    }

    self.hold();
    self.show();
    let report = self.report(SystemTime::now());
    // A rule saying no is no refusal: the status says why.
    let held = self.held.is_some() || self.why_not().is_some();
    let refused = if by_hand { self.session.is_none() } else { wakes && !held };
    if refused { format!("error: {report}") } else { report }
  }

  /// A command's connection closed: its hold ends.
  fn on_closed(&mut self, conn: u64) {
    self.holds.retain(|&(_, c), _| c != conn);
    self.hold();
    self.show();
  }

  /// The holds' assertion as they and the rules say, after those that are due end; the next end
  /// timed, as a session's.
  fn hold(&mut self) {
    let now = SystemTime::now();
    self.holds.retain(|_, until| until.is_none_or(|until| until > now));
    if self.holds.is_empty() || self.why_not().is_some() {
      self.held = None;
    } else if self.held.is_none() {
      self.held = Assertion::new(!self.settings.display_sleeps);
    }

    self.hold_timer += 1;
    let Some(next) = self.holds.values().flatten().min() else { return };
    let timer = self.hold_timer;
    let left = next.duration_since(now).unwrap_or_default();
    self.proxy.after(early(left), move |_| {
      with_state(|state| {
        if state.hold_timer == timer {
          state.hold();
          state.show();
        }
      });
    });
  }

  /// `awake status`: the menu's status, then a line per hold.
  fn report(&self, now: SystemTime) -> String {
    let mut out = self.status(now) + "\n";
    for ((name, conn), until) in &self.holds {
      let what = match (until, conn) {
        (Some(until), _) => format!("{} left", left(*until, now)),
        (None, 0) => "until released".into(),
        (None, _) => "while the command runs".into(),
      };

      out += &format!("{name}: {what}\n");
    }

    out
  }

  fn stop(&mut self, why: Option<String>) {
    self.session = None;
    self.stopped = why;
    self.timer += 1;
    self.show();
  }

  /// The session's end, after `length`: the queue's clock stops while the Mac sleeps, so the end
  /// is checked against the wall clock and waited for again if it is not there yet.
  fn wait(&self, length: Duration) {
    let timer = self.timer;
    self.proxy.after(early(length), move |_| {
      with_state(|state| {
        if state.timer == timer {
          state.expire();
        }
      });
    });
  }

  /// Ends a timed session that is due, or waits for the rest of it.
  fn expire(&mut self) {
    let Some(until) = self.session.as_ref().and_then(|s| s.until) else { return };
    match until.duration_since(SystemTime::now()) {
      Ok(left) if !left.is_zero() => self.wait(left),
      _ => self.stop(None),
    }
  }

  /// The time left shown next changes: in the open menu on the second, beside the cup
  /// on the minute. The tick shows it, and arms the next.
  fn countdown(&mut self, now: SystemTime) {
    self.tick += 1;
    let unit = match (self.menu_open, self.settings.show_left) {
      (true, _) => Duration::from_secs(1),
      (false, true) => minutes(1),
      (false, false) => return,
    };

    let Some(until) = self.session.as_ref().and_then(|s| s.until) else { return };
    let tick = self.tick;
    // The clock's seconds are rounded up: a number changes as the time left passes a whole
    // second, one less than the unit's multiple.
    let left = until.duration_since(now).unwrap_or_default();
    let left = left.saturating_sub(Duration::from_secs(1));
    let next = match left.as_nanos() % unit.as_nanos() {
      0 => unit,
      rest => Duration::from_nanos(rest as u64),
    };

    self.proxy.after(early(next), move |_| {
      with_state(|state| {
        if state.tick == tick {
          state.show();
        }
      });
    });
  }

  /// A check for updates, the person's or an automatic one when [`Updater::due`], on a thread of
  /// its own; its answer comes back to the [`Updater`].
  fn check(&mut self, manual: bool) {
    if !manual && !self.updater.due(SystemTime::now()) || !self.updater.check(manual) {
      return;
    }

    let proxy = self.proxy.clone();
    thread::spawn(move || {
      let found = updater::found(shipo::check(&updater::FEED, VERSION));
      if let Err(e) = &found {
        tracing::warn!("update check: {e}");
      }

      proxy.post(move |_| {
        with_state(|state| {
          state.updater.checked(found, SystemTime::now());
          state.show();
        });
      });
    });

    self.show();
  }

  /// Downloads the update on offer on a thread of its own and starts its installer, which puts it
  /// in once the app has quit and relaunches it; a failure shows in the menu and About.
  fn install(&mut self) {
    let Some(release) = self.updater.install() else { return };
    let proxy = self.proxy.clone();
    thread::spawn(move || {
      let mut at = 0;
      let staged = shipo::download(&release, |done, total| {
        let done = updater::percent(done, total);
        if done != at {
          at = done;
          proxy.post(move |_| with_state(|state| state.on_progress(done)));
        }
      });

      let installed = staged.and_then(shipo::install);
      proxy.post(move |_| with_state(|state| state.on_installed(installed)));
    });

    self.show();
  }

  fn on_progress(&mut self, done: u8) {
    self.updater.progress(done);
    self.show();
  }

  /// The installer runs and waits for the app to quit, or why it didn't start.
  fn on_installed(&mut self, installed: shipo::Result<()>) {
    match installed {
      // At once: the installer gives up after 30 s, and AppKit's quit waits for an open menu to
      // close. The assertion ends with the process.
      Ok(()) => std::process::exit(0),
      Err(e) => {
        tracing::warn!("update: {e}");
        self.updater.failed(updater::why(&e));
        self.show();
      }
    }
  }

  /// Ends the session when a rule says so, unless it was forced: then the rules wait until what
  /// they object to clears (the adapter plugged in), and count again from there.
  fn enforce(&mut self) {
    let why = self.why_not();
    let Some(session) = &mut self.session else { return };
    match (why, &session.forced) {
      (None, _) => session.forced = None,
      (Some(why), Some(_)) => session.forced = Some(why),
      (Some(why), None) => self.stop(Some(why)),
    }
  }

  fn why_not(&self) -> Option<String> {
    rule(&self.settings, power::low_power_mode(), power::battery())
  }

  /// What the window shows besides the settings, read now.
  fn shown(&self) -> Shown {
    Shown {
      settings: self.settings,
      battery: power::battery().is_some(),
      login: autostart::state(&INFO),
      update: self.updater.showing().cloned(),
    }
  }

  /// The menu's first line: on or off and why, the time left, the holds.
  fn status(&self, now: SystemTime) -> String {
    let mut status = match (&self.session, &self.held, &self.stopped) {
      (Some(session), ..) => {
        let mut status = String::from("Keeping awake");
        if let Some(why) = &session.forced {
          status += &format!(" despite {why}");
        }

        if let Some(until) = session.until {
          status += &format!(" · {} left", left(until, now));
        }

        status
      }
      (None, Some(_), _) => "Keeping awake".into(),
      (None, None, Some(why)) => format!("Turned off · {why}"),
      (None, None, None) => "Off".into(),
    };

    if !self.holds.is_empty() {
      let mut names: Vec<_> = self.holds.keys().map(|(name, _)| name.as_str()).collect();
      names.dedup();
      let names = names.join(", ");
      match (&self.held, self.why_not()) {
        (Some(_), _) => status += &format!(" · for {names}"),
        (None, Some(why)) => status += &format!(" · not for {names}: {why}"),
        (None, None) => status += &format!(" · not for {names}: the system refused"),
      }
    }

    status
  }

  fn show(&mut self) {
    let now = SystemTime::now();
    let status = self.status(now);
    let on = self.session.is_some() || self.held.is_some();
    let (icon, description) =
      if on { ("cup.and.saucer.fill", "keeping awake") } else { ("cup.and.saucer", "off") };

    if self.icon != icon {
      self.icon = icon;
      self.tray.set_icon(appkit::symbol(&[icon]));
    }

    let until = self.session.as_ref().and_then(|s| s.until);
    let title = match (self.settings.show_left, until) {
      (true, Some(until)) => short(until, now),
      _ => String::new(),
    };

    if self.title != title {
      self.tray.set_title(&title);
      self.title = title;
    }

    self.tray.set_tooltip(&format!("{}: {description}", INFO.name));
    // The same items as before: an open menu changes in place.
    let length = self.session.as_ref().and_then(|s| s.length);
    // An item come or gone would wait for the menu to close, the countdown with it: the item comes
    // or goes then, and the open menu keeps the old one, disabled.
    let mut update = update_item(self.updater.showing());
    if self.menu_open && update.is_some() != self.update_item.is_some() {
      update = self.update_item.clone().map(|item| item.enabled(false));
    }

    self.update_item = update.clone();
    self.tray.set_menu(&menu(&status, on, length, update));

    if let Some(window) = &self.window {
      let shown = self.shown();
      window.with(|form| form.shown = shown);
    }

    self.countdown(now);
  }
}

// MARK: Rules

/// Why the Mac can't stay awake, by the settings: Low Power Mode, a battery at or below the floor,
/// or running on battery at all.
fn rule(settings: &Settings, low_power: bool, battery: Option<Battery>) -> Option<String> {
  if settings.off_in_low_power && low_power {
    return Some("Low Power Mode".into());
  }

  let battery = battery.filter(|b| b.draining)?;
  if settings.floor_on && battery.percent <= settings.floor {
    return Some(format!("low battery ({}%)", battery.percent));
  }

  settings.off_unplugged.then(|| "battery power".into())
}

/// The time from `now` until `until` as a clock counts down: `59:42`, `2:59:42`.
fn left(until: SystemTime, now: SystemTime) -> String {
  let secs = secs_left(until, now);
  match secs / 3600 {
    0 => format!("{}:{:02}", secs / 60, secs % 60),
    h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
  }
}

/// The time left as the menu bar shows it: the hours and minutes of the menu's clock, `15m`,
/// `1h`, `7h 59m`, and `<1m` for its last minute.
fn short(until: SystemTime, now: SystemTime) -> String {
  let m = secs_left(until, now) / 60;
  match (m / 60, m % 60) {
    (0, 0) => "<1m".into(),
    (0, m) => format!("{m}m"),
    (h, 0) => format!("{h}h"),
    (h, m) => format!("{h}h {m}m"),
  }
}

/// Whole seconds from `now` until `until`, rounded up, so that a clock starts at the length chosen.
fn secs_left(until: SystemTime, now: SystemTime) -> u64 {
  let left = until.duration_since(now).unwrap_or_default();
  left.as_secs() + u64::from(left.subsec_nanos() > 0)
}

// MARK: Launch

fn main() {
  // First: started by an update, the binary is its installer and exits here.
  let updated = shipo::finish();

  // With arguments, the command line; the app has none.
  let args: Vec<_> = std::env::args_os().skip(1).collect();
  if !args.is_empty() {
    std::process::exit(cli::main(&args));
  }

  tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
    .with_writer(std::io::stderr)
    .init();

  if let Some(e) = &updated {
    tracing::warn!("the update that started this app failed: {e}");
  }

  tracing::info!(battery = ?power::battery(), low_power = power::low_power_mode(), "awake");
  let why = updated.map(|e| updater::why(&e));
  nimbo::app(INFO, move |main| start(main, why));
}

/// The app, `failed` why the update that relaunched it failed.
fn start(main: &mut Main, failed: Option<String>) {
  let tray = Tray::new(appkit::symbol(&["cup.and.saucer"]), &format!("{}: off", INFO.name));
  tray.on_open(|_, _| with_state(|state| state.on_menu(true)));
  tray.on_close(|_, _| with_state(|state| state.on_menu(false)));
  tray.on_item(|main, _, action| with_state(|state| state.on_tray(main, action)));
  main.on_reopen(|main| with_state(|state| state.open_settings(main)));
  // Awake Dev and CI's builds between releases take no update, unless faked.
  let updates = !cfg!(debug_assertions)
    && !updater::dev(VERSION)
    && shipo::supported().inspect_err(|e| tracing::info!("updates: {e}")).is_ok();

  let mut updater = Updater::new(updates || shipo::faking());
  if let Some(why) = failed {
    updater.relaunched(why);
  }

  let state = State {
    tray,
    proxy: main.proxy(),
    window: None,
    settings: Settings::load(),
    session: None,
    holds: BTreeMap::new(),
    held: None,
    hold_timer: 0,
    stopped: None,
    timer: 0,
    menu_open: false,
    tick: 0,
    icon: "cup.and.saucer",
    title: String::new(),
    updater,
    update_item: None,
    _watch: None,
  };
  STATE.set(Some(state));
  cli::listen(main.proxy(), on_request, |conn| with_state(|state| state.on_closed(conn)));

  let watch = power::watch(|change| with_state(|state| state.on_power(change)));
  with_state(|state| {
    state._watch = watch;
    if state.settings.on_launch {
      state.start(None, false);
    }

    state.check(false);
    state.show();
  });
}

/// [`State::on_request`], for the command line's server.
fn on_request(request: Request, conn: u64) -> String {
  let mut reply = "error: Awake is busy, try again".to_string();
  with_state(|state| reply = state.on_request(request, conn));
  reply
}

#[cfg(test)]
mod tests {
  use super::*;

  fn battery(percent: u32, draining: bool) -> Option<Battery> {
    Some(Battery { percent, draining })
  }

  #[test]
  fn awake_dev_is_an_app_of_its_own() {
    // What installs and macOS know Awake by: never changed.
    assert_eq!((RELEASE.id, RELEASE.name), ("me.vladkens.awake", "Awake"));
    assert_eq!((DEV.id, DEV.name), ("me.vladkens.awake-dev", "Awake Dev"));
    assert_eq!(INFO.id, RELEASE.id, "a release build, as `make test`'s, is Awake");
  }

  #[test]
  fn time_left_counts_down_like_a_clock() {
    let now = SystemTime::UNIX_EPOCH;
    let at = |millis| left(now + Duration::from_millis(millis), now);
    assert_eq!(at(0), "0:00");
    assert_eq!(at(1), "0:01");
    assert_eq!(at(59_400), "1:00");
    assert_eq!(at(3_600_000), "1:00:00");
    assert_eq!(at(3_599_001), "1:00:00");
    assert_eq!(at(3 * 3_600_000 - 18_000), "2:59:42");
    assert_eq!(left(now, now + HOUR), "0:00");
  }

  #[test]
  fn the_menu_bar_reads_as_the_menus_clock() {
    let now = SystemTime::UNIX_EPOCH;
    let at = |millis| short(now + Duration::from_millis(millis), now);
    assert_eq!(at(15 * 60_000), "15m");
    assert_eq!(at(15 * 60_000 - 500), "15m");
    assert_eq!(at(15 * 60_000 - 1000), "14m");
    assert_eq!(at(29 * 60_000 + 24_000), "29m");
    assert_eq!(at(60_000), "1m");
    assert_eq!(at(59_000), "<1m");
    assert_eq!(at(0), "<1m");
    assert_eq!(at(3_600_000), "1h");
    assert_eq!(at(3_599_000), "59m");
    assert_eq!(at(8 * 3_600_000 - 60_000), "7h 59m");
  }

  #[test]
  fn lengths_read_as_the_menu_names_them() {
    let titles: Vec<_> = LENGTHS.into_iter().map(title).collect();
    let want = ["15 Minutes", "30 Minutes", "1 Hour", "2 Hours", "4 Hours", "8 Hours"];
    assert_eq!(titles, want);
  }

  #[test]
  fn low_power_mode_ends_it_only_when_asked() {
    let asked = Settings { off_in_low_power: true, ..Settings::default() };
    assert_eq!(rule(&asked, true, None).as_deref(), Some("Low Power Mode"));
    assert_eq!(rule(&asked, false, None), None);
    assert_eq!(rule(&Settings::default(), true, None), None);
  }

  #[test]
  fn the_floor_ends_it_on_battery_only() {
    let floor = Settings { floor_on: true, floor: 20, ..Settings::default() };
    assert_eq!(rule(&floor, false, battery(20, true)).as_deref(), Some("low battery (20%)"));
    assert_eq!(rule(&floor, false, battery(21, true)), None);
    assert_eq!(rule(&floor, false, battery(5, false)), None);
    assert_eq!(rule(&floor, false, None), None);
    assert_eq!(rule(&Settings { floor_on: false, ..floor }, false, battery(5, true)), None);
  }

  #[test]
  fn unplugging_ends_it_when_asked() {
    let asked = Settings { off_unplugged: true, ..Settings::default() };
    assert_eq!(rule(&asked, false, battery(90, true)).as_deref(), Some("battery power"));
    assert_eq!(rule(&asked, false, battery(90, false)), None);
    assert_eq!(rule(&Settings::default(), false, battery(90, true)), None);
  }
}
