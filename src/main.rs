// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! Awake: keeps the Mac awake from the menu bar, as KeepingYouAwake does. A click on the cup opens
//! the menu: on until turned off or for one of [`LENGTHS`]. The settings window (Settings… in the
//! menu, or the app opened again) has the rest: launch at login, on at launch, the display left to
//! sleep, off when the screen locks, the time left beside the cup, and the battery's rules, which a
//! session turned on by hand outlives. The status item, the menu and the window are nimbo's; what
//! nimbo lacks is AppKit's, in `appkit.rs`. Updates come in place from GitHub's releases, offered
//! in a window of their own as Sparkle's (`updater.rs`). Run with arguments, the binary is the
//! command line (`cli.rs`), which asks the running app: on, off, the status, and holds by name or
//! for a command, which keep the Mac awake beside the session. The menu and the window speak the
//! Mac's language from nimbo's gettext catalogs ([`LOCALES`]); the command line speaks English.
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
use std::fmt::Display;
use std::thread;
use std::time::{Duration, SystemTime};

use cli::Request;
use nimbo::tray::{Item, Menu, Tray};
use nimbo::{
  AppInfo, Localizations, Localized, Main, Presence, Proxy, Shortcut, Window, autostart, t, tn,
};
use power::{Assertion, Battery, Change};
use settings::Settings;
use tracing_subscriber::EnvFilter;
use updater::{Act, UpdateWindow, Updater};
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
/// The app's translations (`make i18n` keeps them): its English texts are the keys.
const LOCALES: Localizations = &[
  ("de", include_str!("../locales/de.po")),
  ("es", include_str!("../locales/es.po")),
  ("fr", include_str!("../locales/fr.po")),
  ("it", include_str!("../locales/it.po")),
  ("ja", include_str!("../locales/ja.po")),
  ("ko", include_str!("../locales/ko.po")),
  ("pl", include_str!("../locales/pl.po")),
  ("pt-BR", include_str!("../locales/pt-BR.po")),
  ("ru", include_str!("../locales/ru.po")),
  ("tr", include_str!("../locales/tr.po")),
  ("uk", include_str!("../locales/uk.po")),
  ("zh-Hans", include_str!("../locales/zh-Hans.po")),
];
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
  /// The update's row: its window, with the release on offer, its download or its failure.
  Update,
  Settings,
  /// Check for Updates…: the update's window, with a check when it has nothing else to show.
  CheckUpdates,
  Quit,
}

/// Awake now.
struct Session {
  _assertion: Assertion,
  /// How long it was started for: the submenu's checkmark.
  length: Option<Duration>,
  /// When it ends by itself.
  until: Option<SystemTime>,
  /// The rules that said no when it was turned on by hand: each waits until it clears.
  waived: Rules,
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
  stopped: Option<Why>,
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
  /// The update's window, while open.
  update_window: Option<Window<UpdateWindow>>,
  /// The update's row in the menu as last set: an open menu keeps it, or its absence.
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

/// The menu while on or off; `length` is the timed session's, `update` the update's row under the
/// status, `check` Check for Updates… where the app can update.
fn menu(
  status: &str,
  on: bool,
  length: Option<Duration>,
  (update, check): (Option<Item<Action>>, Option<Item<Action>>),
) -> Menu<Action> {
  let lengths = LENGTHS.iter().enumerate();
  let lengths = lengths.map(|(i, &l)| Item::toggle(Action::For(i), &title(l), length == Some(l)));
  let mut items = vec![Item::full(Action::Status, status).enabled(false)];
  items.extend(update);
  items.extend([
    Item::Divider,
    Item::button(Action::Toggle, toggle(on)),
    Item::submenu(&t!("Keep Awake For"), Menu::new(lengths.collect())),
    Item::Divider,
    Item::button(Action::Settings, &t!("Settings…")).key(Shortcut::char(',').cmd()),
  ]);
  items.extend(check);
  let quit = Item::button(Action::Quit, &t!("Quit {name}", name = INFO.name));
  items.extend([Item::Divider, quit.key(Shortcut::char('q').cmd())]);
  Menu::new(items)
}

/// The update's row while it has news: a release on offer, its download, the restart, a failure.
fn update_item(state: Option<&updater::State>) -> Option<Item<Action>> {
  use updater::{State, shown};
  let label = match state? {
    State::Available(r) => t!("Update Available: {version}…", version = shown(&r.version)),
    State::Downloading(_, done) => t!("Downloading Update: {done}%", done),
    State::Restarting(_) => t!("Restarting to Update…").to_string(),
    State::Failed(..) | State::Relaunched(_) => t!("Update Failed…").to_string(),
    State::Idle | State::Checking | State::UpToDate => return None,
  };

  Some(Item::button(Action::Update, &label))
}

/// Check for Updates…, which opens the update's window: Checking while the person's check runs.
fn check_item(state: Option<&updater::State>) -> Option<Item<Action>> {
  let label = match state? {
    updater::State::Checking => t!("Checking for Updates…"),
    _ => t!("Check for Updates…"),
  };

  Some(Item::button(Action::CheckUpdates, label.as_str()))
}

const fn minutes(m: u64) -> Duration {
  Duration::from_secs(m * 60)
}

fn toggle(on: bool) -> &'static str {
  if on { t!("Turn Off").as_str() } else { t!("Keep Awake Indefinitely").as_str() }
}

/// A length as the submenu names it: `15 Minutes`, `1 Hour`, `8 Hours`.
fn title(length: Duration) -> String {
  match (length.as_secs() / 3600, length.as_secs() / 60) {
    (0, m) => tn!("{n} Minute", "{n} Minutes", m),
    (h, _) => tn!("{n} Hour", "{n} Hours", h),
  }
}

/// The language a text is said in: the app's, or English, the command line's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lang {
  App,
  English,
}

/// `text` in `lang`, its `{name}`s filled from `args`: the menu's status and the command line's
/// are the same texts.
fn say(lang: Lang, text: Localized, args: &[(&str, &dyn Display)]) -> String {
  let mut rest = if lang == Lang::App { text.as_str() } else { text.key() };
  // One pass: a value that holds a `{name}` itself, as a hold's name may, stays as it is.
  let mut said = String::new();
  while let Some((before, after)) = rest.split_once('{')
    && let Some((key, after)) = after.split_once('}')
  {
    said += before;
    match args.iter().find(|(name, _)| *name == key) {
      Some((_, v)) => said += &v.to_string(),
      None => said += &format!("{{{key}}}"),
    }

    rest = after;
  }

  said + rest
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
      Action::Update | Action::CheckUpdates => self.open_update(main),
    }
  }

  /// The menu opens or closes: built anew, as the time left, the login item's state and the
  /// battery change while it is closed, and the countdown's pace changes.
  fn on_menu(&mut self, open: bool) {
    self.menu_open = open;
    if open {
      self.check();
    }

    self.show();
  }

  fn on_form(&mut self, main: &mut Main, event: FormEvent) {
    match event {
      FormEvent::Login(on) => match autostart::set(&INFO, on) {
        Ok(autostart::State::RequiresApproval) => autostart::open_settings(),
        Ok(_) => {}
        Err(e) => tracing::warn!("launch at login: {e}"),
      },
      FormEvent::CheckUpdates => self.open_update(main),
      FormEvent::Changed(settings) => {
        let display = settings.display_sleeps != self.settings.display_sleeps;
        self.settings = settings;
        // The assertion of the other kind, for the same session.
        if display && let Some(session) = self.session.take() {
          match Assertion::new(!settings.display_sleeps) {
            Some(assertion) => self.session = Some(Session { _assertion: assertion, ..session }),
            None => self.stop(Some(Why::Refused)),
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
      // The queue's clock stopped while the Mac slept, or the wall clock moved: the ends are timed
      // again.
      Change::Woke | Change::Clock => {
        self.timer += 1;
        self.expire();
        self.check();
        self.hold();
      }
      Change::Locked if self.settings.off_on_lock && self.session.is_some() => {
        self.stop(Some(Why::Locked));
      }
      Change::Locked => {}
    }

    self.show();
  }

  fn open_settings(&mut self, main: &mut Main) {
    if self.window.is_none() {
      let proxy = self.proxy.clone();
      let on = move |event| proxy.post(move |main| with_state(|state| state.on_form(main, event)));
      let (cfg, form) = Form::new(self.shown(), on);
      let window = main.open(&cfg, form);
      window.on_close(|_| with_state(|state| state.window = None));
      self.window = Some(window);
    }

    self.show();
    if let Some(window) = &self.window {
      window.front();
    }
  }

  /// A session of `length`, or until turned off. By hand it starts whatever the rules say, and
  /// those wait until what they object to clears; at launch they decide.
  fn start(&mut self, length: Option<Duration>, by_hand: bool) {
    let rules = self.rules();
    if !rules.is_empty() && !by_hand {
      self.stop(rules.why());
      return;
    }

    let Some(assertion) = Assertion::new(!self.settings.display_sleeps) else {
      self.stop(Some(Why::Refused));
      return;
    };

    self.timer += 1;
    let until = length.map(|length| SystemTime::now() + length);
    self.session = Some(Session { _assertion: assertion, length, until, waived: rules });
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
    let mut added = None;
    match request {
      Request::On(length) => self.start(length, true),
      Request::Off => self.off(),
      Request::Status => {}
      Request::Hold(name, length) => {
        let until = length.map(|length| SystemTime::now() + length);
        let before = self.holds.insert((name.clone(), 0), until);
        added = Some(((name, 0), before));
      }
      Request::Release(name) => _ = self.holds.remove(&(name, 0)),
      Request::Run(name) => {
        self.holds.insert((name.clone(), conn), None);
        added = Some(((name, conn), None));
      }
    }

    self.hold();
    self.show();
    let report = self.report(SystemTime::now());
    // A rule saying no is no refusal: the status says why.
    let held = self.held.is_some() || self.why_not().is_some();
    let refused = if by_hand { self.session.is_none() } else { wakes && !held };
    if !refused {
      return report;
    }

    // Told no, the command line gives up on the hold: kept, it would hold later, unseen. A hold
    // renewed is as it was.
    if let Some((key, before)) = added {
      match before {
        Some(until) => _ = self.holds.insert(key, until),
        None => _ = self.holds.remove(&key),
      }

      self.hold();
      self.show();
    }

    format!("error: {report}")
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
    let mut out = self.status(now, Lang::English) + "\n";
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

  fn stop(&mut self, why: Option<Why>) {
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

  /// An automatic check for updates when one is [`Updater::due`].
  fn check(&mut self) {
    if self.updater.due(SystemTime::now()) && self.updater.check(false) {
      self.spawn_check();
    }
  }

  /// The check, on a thread of its own; its answer comes back to the [`Updater`].
  fn spawn_check(&self) {
    let proxy = self.proxy.clone();
    thread::spawn(move || {
      let found = updater::found(shipo::check(&updater::FEED, VERSION));
      if let Err(e) = &found {
        tracing::warn!("update check: {e}");
      }

      proxy.post(move |_| {
        with_state(|state| {
          let skipped = settings::skipped();
          state.updater.checked(found, skipped.as_deref(), SystemTime::now());
          state.show();
        });
      });
    });
  }

  /// The update's window, brought to the front if open, with a check when there is nothing else to
  /// show.
  fn open_update(&mut self, main: &mut Main) {
    if self.updater.open() {
      self.spawn_check();
    }

    match &self.update_window {
      Some(window) => window.front(),
      None => {
        let proxy = self.proxy.clone();
        let chosen = move |act| proxy.post(move |_| with_state(|state| state.on_update(act)));
        let window =
          main.open(&updater::config(), UpdateWindow::new(self.updater.state.clone(), chosen));
        window.on_close(|_| {
          with_state(|state| {
            state.update_window = None;
            state.updater.closed();
            state.show();
          });
        });
        window.front();
        self.update_window = Some(window);
      }
    }

    self.show();
  }

  /// From the update's window.
  /// Install, or Retry: the download, or a check after a failed one or a failed relaunch;
  /// `false` when there is nothing to do, the app can't update any more.
  fn install(&mut self) -> bool {
    if let Some(release) = self.updater.install() {
      self.download(release);
    } else if self.updater.check(true) {
      self.spawn_check();
    }

    !matches!(self.updater.state, updater::State::Relaunched(_))
  }

  fn on_update(&mut self, act: Act) {
    let mut close = false;
    match act {
      Act::Install | Act::Retry => close = !self.install(),
      Act::Later => {
        self.updater.later();
        close = true;
      }
      Act::Skip => {
        if let Some(version) = self.updater.skip() {
          settings::skip(&version);
        }

        close = true;
      }
      Act::Notes => {
        if let updater::State::Available(r) = &self.updater.state
          && let Some(notes) = &r.notes
          && let Err(e) = nimbo::open::url(notes)
        {
          tracing::warn!("the release notes: {e}");
        }
      }
      Act::Close => close = true,
    }

    self.show();
    // From the loop, the state let go: the window's close calls back into it.
    if close {
      self.proxy.post(|_| {
        let mut window = None;
        with_state(|state| window = state.update_window.clone());
        if let Some(window) = window {
          window.close();
        }
      });
    }
  }

  /// Downloads `release` on a thread of its own and starts its installer, which puts it in once
  /// the app has quit and relaunches it; a failure shows in the menu and the update's window.
  fn download(&mut self, release: shipo::Release) {
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

      let installed = staged.and_then(|staged| {
        proxy.post(|_| with_state(State::on_downloaded));
        shipo::install(staged)
      });
      proxy.post(move |_| with_state(|state| state.on_installed(installed)));
    });
  }

  fn on_progress(&mut self, done: u8) {
    self.updater.progress(done);
    self.show();
  }

  fn on_downloaded(&mut self) {
    self.updater.downloaded();
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

  /// Ends the session when a rule says so, unless it was turned on despite that rule: then the rule
  /// waits until what it objects to clears (the adapter plugged in), and counts again from there.
  fn enforce(&mut self) {
    let rules = self.rules();
    let Some(session) = &mut self.session else { return };
    match rules.without(session.waived) {
      new if new.is_empty() => session.waived = rules.and(session.waived),
      new => self.stop(new.why()),
    }
  }

  fn rules(&self) -> Rules {
    Rules::new(&self.settings, power::low_power_mode(), power::battery())
  }

  fn why_not(&self) -> Option<Why> {
    self.rules().why()
  }

  /// What the window shows besides the settings, read now.
  fn shown(&self) -> Shown {
    Shown {
      settings: self.settings,
      battery: power::battery().is_some(),
      login: autostart::state(&INFO),
      updates: self.updater.showing().is_some(),
    }
  }

  /// The menu's first line, and the command line's status: on or off and why, the time left, the
  /// holds.
  fn status(&self, now: SystemTime, lang: Lang) -> String {
    let mut parts = match (&self.session, &self.held, self.stopped) {
      (Some(session), ..) => {
        let on = match session.waived.why() {
          Some(why) => say(lang, t!("Keeping awake despite {why}"), &[("why", &why.text(lang))]),
          None => say(lang, t!("Keeping awake"), &[]),
        };

        let left = session.until.map(|until| left(until, now));
        // Translators: {time} is a clock counting down: 59:42, 2:59:42.
        let left = left.map(|time| say(lang, t!("{time} left"), &[("time", &time)]));
        [Some(on), left].into_iter().flatten().collect()
      }
      (None, Some(_), _) => vec![say(lang, t!("Keeping awake"), &[])],
      // Translators: the menu's first line after the app turned off by itself, before why.
      (None, None, Some(why)) => vec![say(lang, t!("Turned off"), &[]), why.text(lang)],
      // Translators: the menu's first line while nothing keeps the Mac awake.
      (None, None, None) => vec![say(lang, t!("Off"), &[])],
    };

    if !self.holds.is_empty() {
      let mut names: Vec<_> = self.holds.keys().map(|(name, _)| name.as_str()).collect();
      names.dedup();
      let names = names.join(", ");
      let held = match (&self.held, self.why_not()) {
        // Translators: {names} are the command line's holds, by the names it gave them.
        (Some(_), _) => say(lang, t!("for {names}"), &[("names", &names)]),
        (None, why) => {
          let why = why.unwrap_or(Why::Refused).text(lang);
          say(lang, t!("not for {names}: {why}"), &[("names", &names), ("why", &why)])
        }
      };

      parts.push(held);
    }

    parts.join(" · ")
  }

  fn show(&mut self) {
    let now = SystemTime::now();
    let status = self.status(now, Lang::App);
    let on = self.session.is_some() || self.held.is_some();
    let icon = if on { "cup.and.saucer.fill" } else { "cup.and.saucer" };

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

    self.tray.set_tooltip(&tooltip(on));
    // The same items as before: an open menu changes in place.
    let length = self.session.as_ref().and_then(|s| s.length);
    // An item come or gone would wait for the menu to close, the countdown with it: the item comes
    // or goes then, and the open menu keeps the old one, disabled.
    let mut update = update_item(self.updater.showing());
    if self.menu_open && update.is_some() != self.update_item.is_some() {
      update = self.update_item.clone().map(|item| item.enabled(false));
    }

    self.update_item = update.clone();
    let check = check_item(self.updater.showing());
    self.tray.set_menu(&menu(&status, on, length, (update, check)));

    if let Some(window) = &self.window {
      let shown = self.shown();
      window.with(|form| form.shown = shown);
    }

    if let Some(window) = &self.update_window {
      let state = self.updater.state.clone();
      window.with(|w| w.state = state);
    }

    self.countdown(now);
  }
}

// MARK: Rules

/// The rules that say the Mac can't stay awake, by the settings: Low Power Mode, a battery below
/// the floor, running on battery at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rules {
  low_power: bool,
  /// The battery's charge, below the floor.
  floor: Option<u32>,
  unplugged: bool,
}

impl Rules {
  fn new(settings: &Settings, low_power: bool, battery: Option<Battery>) -> Self {
    let battery = battery.filter(|b| b.draining);
    let floor = battery.map(|b| b.percent).filter(|&p| settings.floor_on && p < settings.floor);
    Self {
      low_power: settings.off_in_low_power && low_power,
      floor,
      unplugged: settings.off_unplugged && battery.is_some(),
    }
  }

  fn is_empty(self) -> bool {
    self == Self::default()
  }

  /// Those of `self` that `other` lacks.
  fn without(self, other: Self) -> Self {
    Self {
      low_power: self.low_power && !other.low_power,
      floor: self.floor.filter(|_| other.floor.is_none()),
      unplugged: self.unplugged && !other.unplugged,
    }
  }

  /// Those of both, with the charge of `self`.
  fn and(self, other: Self) -> Self {
    Self {
      low_power: self.low_power && other.low_power,
      floor: self.floor.filter(|_| other.floor.is_some()),
      unplugged: self.unplugged && other.unplugged,
    }
  }

  /// The first of them.
  fn why(self) -> Option<Why> {
    if self.low_power {
      return Some(Why::LowPower);
    }

    if let Some(percent) = self.floor {
      return Some(Why::Floor(percent));
    }

    self.unplugged.then_some(Why::Unplugged)
  }
}

/// Why the Mac can't stay awake, or why the app turned off by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
  LowPower,
  /// The battery's charge, below the floor.
  Floor(u32),
  Unplugged,
  Locked,
  /// The system refused the assertion.
  Refused,
}

impl Why {
  /// As the status says it, after "Turned off" or "despite".
  fn text(self, lang: Lang) -> String {
    match self {
      // Translators: why the Mac can't stay awake, after "Turned off ·" and in "Keeping awake
      // despite {why}"; these and the next ones.
      Why::LowPower => say(lang, t!("Low Power Mode"), &[]),
      Why::Floor(percent) => say(lang, t!("low battery ({percent}%)"), &[("percent", &percent)]),
      Why::Unplugged => say(lang, t!("battery power"), &[]),
      Why::Locked => say(lang, t!("screen locked"), &[]),
      Why::Refused => say(lang, t!("the system refused"), &[]),
    }
  }
}

/// The status item's tooltip, which VoiceOver reads too.
fn tooltip(on: bool) -> String {
  if on {
    t!("{name}: keeping awake", name = INFO.name)
  } else {
    t!("{name}: off", name = INFO.name)
  }
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

  // Run bare beside the app (`awake` typed in a terminal): the help, not a second app.
  if cli::running() {
    std::process::exit(cli::main(&["help".into()]));
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
  main.set_localizations(LOCALES);
  let tray = Tray::new(appkit::symbol(&["cup.and.saucer"]), &tooltip(false));
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
    update_window: None,
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

    state.check();
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

  /// An update shows under the status; Check for Updates… sits under Settings…, and Checking while
  /// the person's check runs. An app that can't update has neither.
  #[test]
  fn an_update_shows_under_the_status() {
    let items = |state: Option<updater::State>| {
      let update = (update_item(state.as_ref()), check_item(state.as_ref()));
      let items = menu("Off", false, None, update).items;
      items.into_iter().map(|i| (i.action().cloned(), i.label().to_owned())).collect::<Vec<_>>()
    };

    let idle = items(Some(updater::State::Idle));
    let ids: Vec<_> = idle[idle.len() - 4..].iter().map(|(a, _)| *a).collect();
    assert_eq!(ids, [Some(Action::Settings), Some(Action::CheckUpdates), None, Some(Action::Quit)]);
    assert_eq!(idle[idle.len() - 3].1, "Check for Updates…");
    assert!(!idle.iter().any(|(a, _)| *a == Some(Action::Update)), "no news");

    let available = items(Some(updater::State::Available(shipo::Release::test("1.2.0"))));
    assert_eq!(available[1], (Some(Action::Update), "Update Available: v1.2.0…".into()));
    let checking = items(Some(updater::State::Checking));
    assert_eq!(checking[checking.len() - 3].1, "Checking for Updates…");

    let off = items(None);
    assert!(!off.iter().any(|(a, _)| matches!(a, Some(Action::Update | Action::CheckUpdates))));
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

  /// In Russian the lengths take its three plurals, and a reason reads in Russian in the menu and
  /// in English on the command line.
  #[test]
  fn the_menu_speaks_russian_and_the_command_line_english() {
    struct Blank;
    impl nimbo::App for Blank {
      fn view(&mut self, _: &mut nimbo::Ui) {}
    }

    // The harness gives its language to this thread.
    let mut h = nimbo::Harness::new(Blank, nimbo::Size::new(1.0, 1.0), 1.0);
    h.set_locale(nimbo::Locale::Tag("ru"), LOCALES);
    let titles: Vec<_> = LENGTHS.into_iter().map(title).collect();
    assert_eq!(titles, ["15 минут", "30 минут", "1 час", "2 часа", "4 часа", "8 часов"]);
    assert_eq!(Why::Floor(19).text(Lang::App), "низкий заряд (19\u{a0}%)");
    assert_eq!(Why::Floor(19).text(Lang::English), "low battery (19%)");
  }

  /// Each catalog has every text translated and none fuzzy: `make i18n` brings them up to the
  /// sources.
  #[test]
  fn every_catalog_is_complete() {
    for (tag, po) in LOCALES {
      for entry in po.split("\n\n").skip(1).filter(|e| !e.trim().is_empty()) {
        assert!(!entry.contains("#, fuzzy"), "{tag}: fuzzy: {entry}");
        let lines: Vec<&str> = entry.lines().collect();
        for (i, line) in lines.iter().enumerate() {
          let next = lines.get(i + 1).is_some_and(|l| l.starts_with('"'));
          let empty = line.starts_with("msgstr") && line.ends_with(" \"\"") && !next;
          assert!(!empty, "{tag}: not translated: {entry}");
        }
      }
    }
  }

  fn rule(settings: &Settings, low_power: bool, battery: Option<Battery>) -> Option<Why> {
    Rules::new(settings, low_power, battery).why()
  }

  #[test]
  fn low_power_mode_ends_it_only_when_asked() {
    let asked = Settings { off_in_low_power: true, ..Settings::default() };
    assert_eq!(rule(&asked, true, None), Some(Why::LowPower));
    assert_eq!(rule(&asked, false, None), None);
    assert_eq!(rule(&Settings::default(), true, None), None);
  }

  #[test]
  fn the_floor_ends_it_on_battery_only() {
    let floor = Settings { floor_on: true, floor: 20, ..Settings::default() };
    assert_eq!(rule(&floor, false, battery(19, true)), Some(Why::Floor(19)));
    assert_eq!(rule(&floor, false, battery(20, true)), None, "below the floor, not at it");
    assert_eq!(rule(&floor, false, battery(5, false)), None);
    assert_eq!(rule(&floor, false, None), None);
    assert_eq!(rule(&Settings { floor_on: false, ..floor }, false, battery(5, true)), None);
  }

  #[test]
  fn unplugging_ends_it_when_asked() {
    let asked = Settings { off_unplugged: true, ..Settings::default() };
    assert_eq!(rule(&asked, false, battery(90, true)), Some(Why::Unplugged));
    assert_eq!(rule(&asked, false, battery(90, false)), None);
    assert_eq!(rule(&Settings::default(), false, battery(90, true)), None);
  }

  #[test]
  fn a_session_on_despite_one_rule_ends_at_another() {
    let settings =
      Settings { floor_on: true, floor: 20, off_in_low_power: true, ..Settings::default() };
    // What enforce() does with the session's waived rules: kept, or the session ends and why.
    let step = |waived: Rules, low_power, battery| {
      let rules = Rules::new(&settings, low_power, battery);
      let new = rules.without(waived);
      if new.is_empty() { Ok(rules.and(waived)) } else { Err(new.why()) }
    };
    let ended = |why| Err(Some(why));

    // Turned on by hand in Low Power Mode, on battery.
    let waived = Rules::new(&settings, true, battery(50, true));
    let waived = step(waived, true, battery(30, true)).expect("only Low Power Mode");
    assert_eq!(waived.why(), Some(Why::LowPower));
    assert_eq!(step(waived, true, battery(19, true)), ended(Why::Floor(19)));
    assert_eq!(step(waived, false, battery(19, true)), ended(Why::Floor(19)));

    // Low Power Mode turned off clears its waiver: on again, it ends the session.
    let waived = step(waived, false, battery(30, true)).expect("no rule");
    assert!(waived.is_empty());
    assert_eq!(step(waived, true, battery(30, true)), ended(Why::LowPower));

    // A waived floor stays waived as the charge falls, and the status shows the charge.
    let waived = Rules::new(&settings, false, battery(15, true));
    let waived = step(waived, false, battery(12, true)).expect("the floor waived");
    assert_eq!(waived.why(), Some(Why::Floor(12)));
    assert_eq!(step(waived, true, battery(12, true)), ended(Why::LowPower));
  }

  #[test]
  fn the_command_line_says_why_in_english() {
    let why = [Why::LowPower, Why::Floor(19), Why::Unplugged, Why::Locked, Why::Refused];
    let want = [
      "Low Power Mode",
      "low battery (19%)",
      "battery power",
      "screen locked",
      "the system refused",
    ];
    assert_eq!(why.map(|why| why.text(Lang::English)), want);
  }

  #[test]
  fn a_hold_named_like_a_placeholder_keeps_its_name() {
    let text = t!("not for {names}: {why}");
    let said = say(Lang::English, text, &[("names", &"backup-{why}"), ("why", &"screen locked")]);
    assert_eq!(said, "not for backup-{why}: screen locked");
  }
}
