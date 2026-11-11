// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! Awake: keeps the Mac awake from the menu bar, as KeepingYouAwake does. A click on the cup opens
//! the menu: on until turned off or for an hour or three. The settings window (Settings… in the
//! menu, or the app opened again) has the rest: launch at login, on at launch, the display left to
//! sleep, and the battery's rules, which a session turned on by hand outlives. The status item,
//! the menu and the window are nimbo's; what nimbo lacks is AppKit's, in `appkit.rs`.
//!
//! Unlike the AppKit version: the settings window has nimbo's System Settings look (switches,
//! notes under the names, why a row is off on an ⓘ beside it) and a fixed title, the app's name;
//! the battery slider has no tick marks, and keys and VoiceOver can't move it (nimbo's arrows step
//! a twentieth of the range, which rounds back to the same 10 %); while the window is open the app
//! menu has only Close Window and Quit, no About or Hide.
//!   make run

mod appkit;
mod power;
mod settings;
mod window;

use std::cell::RefCell;
use std::time::{Duration, SystemTime};

use nimbo::tray::{Item, Menu, Tray};
use nimbo::{AppInfo, Main, Presence, Proxy, Shortcut, Window, autostart};
use power::{Assertion, Battery, Change};
use settings::Settings;
use window::{Form, FormEvent, Shown};

/// The app this build is: [RELEASE], or [DEV] built with `AWAKE_FLAVOR=dev` (build.rs).
const INFO: AppInfo = if cfg!(awake_dev) { DEV } else { RELEASE };
/// Awake, as released and installed: its id is in every install (settings, login item).
const RELEASE: AppInfo =
  AppInfo { id: "me.vladkens.awake", name: "Awake", presence: Presence::Accessory };
/// Awake Dev, `make run`'s: an app of its own beside the installed Awake, with its own settings,
/// login item and instance, all by its id.
const DEV: AppInfo =
  AppInfo { id: "me.vladkens.awake-dev", name: "Awake Dev", presence: Presence::Accessory };
const HOUR: Duration = Duration::from_secs(60 * 60);
/// The menu's sessions: the item's title and how long.
const MODES: [(&str, Option<Duration>); 3] = [
  ("Keep Awake", None),
  ("For 1 Hour", Some(HOUR)),
  ("For 3 Hours", Some(Duration::from_secs(3 * 60 * 60))),
];

/// What the menu's items do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
  /// The status line: nothing.
  Status,
  /// One of [`MODES`].
  Mode(usize),
  Settings,
  Quit,
}

/// Awake now.
struct Session {
  _assertion: Assertion,
  /// The index in [`MODES`] of the item that started it: the checkmark's.
  mode: usize,
  /// When it ends by itself.
  until: Option<SystemTime>,
  /// Turned on by hand while a rule said no (why): the rules wait until that clears.
  forced: Option<String>,
}

struct State {
  tray: Tray<Action>,
  proxy: Proxy,
  /// While open: opened when asked, dropped when closed.
  window: Option<Window<Form>>,
  settings: Settings,
  session: Option<Session>,
  /// Why the app last turned off by itself (not the user), until it is on again.
  stopped: Option<String>,
  /// The session's timer: one that fires for an earlier session finds another number.
  timer: u64,
  /// The menu is open: its countdown ticks.
  menu_open: bool,
  /// The countdown's tick: one of an earlier opening finds another number.
  tick: u64,
  /// The icon's symbol: drawn again only when it changes.
  icon: &'static str,
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

fn menu(status: &str, mode: Option<usize>) -> Menu<Action> {
  let mut items = vec![Item::full(Action::Status, status).enabled(false), Item::Divider];
  for (i, (title, _)) in MODES.iter().enumerate() {
    items.push(Item::toggle(Action::Mode(i), title, mode == Some(i)));
  }

  items.extend([
    Item::Divider,
    Item::button(Action::Settings, "Settings…").key(Shortcut::char(',').cmd()),
    Item::button(Action::Quit, &format!("Quit {}", INFO.name)).key(Shortcut::char('q').cmd()),
  ]);

  Menu::new(items)
}

// MARK: State

impl State {
  fn on_tray(&mut self, main: &mut Main, action: Action) {
    match action {
      Action::Status => {}
      Action::Settings => self.open_settings(main),
      Action::Quit => main.quit(),
      // The item checked turns it off; another one starts its own session.
      Action::Mode(mode) if self.session.as_ref().is_some_and(|s| s.mode == mode) => {
        self.stop(None)
      }
      Action::Mode(mode) => self.start(mode, true),
    }
  }

  /// Only the countdown starts or stops: the menu is built as it opens and once it closes.
  fn on_menu(&mut self, open: bool) {
    self.menu_open = open;
    self.tick += 1;
    self.countdown();
  }

  fn on_form(&mut self, event: FormEvent) {
    match event {
      FormEvent::Login(on) => match autostart::set(&INFO, on) {
        Ok(autostart::State::RequiresApproval) => autostart::open_settings(),
        Ok(_) => {}
        Err(e) => tracing::warn!("launch at login: {e}"),
      },
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

        self.settings.save();
        self.enforce();
      }
    }

    self.show();
  }

  fn on_power(&mut self, change: Change) {
    match change {
      Change::Battery | Change::LowPowerMode => self.enforce(),
      // The queue's clock stopped while the Mac slept: the session's end is timed again.
      Change::Woke => {
        self.timer += 1;
        self.expire();
      }
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

  /// A session of `MODES[mode]`. By hand it starts whatever the rules say, and they wait until
  /// what they object to clears; at launch they decide.
  fn start(&mut self, mode: usize, by_hand: bool) {
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
    let length = MODES[mode].1;
    let until = length.map(|length| SystemTime::now() + length);
    self.session = Some(Session { _assertion: assertion, mode, until, forced: why });
    self.stopped = None;

    if let Some(length) = length {
      self.wait(length);
    }

    self.show();
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

  /// While the menu is open, the time left in it ticks each second, on the second's change.
  fn countdown(&mut self) {
    let until = self.session.as_ref().and_then(|s| s.until);
    let (true, Some(until)) = (self.menu_open, until) else { return };

    self.tick += 1;
    let tick = self.tick;
    let left = until.duration_since(SystemTime::now()).unwrap_or_default();
    let next = match left.subsec_nanos() {
      0 => Duration::from_secs(1),
      nanos => Duration::from_nanos(nanos.into()),
    };

    self.proxy.after(next, move |_| {
      with_state(|state| {
        if state.tick == tick {
          state.show();
          state.countdown();
        }
      });
    });
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
    }
  }

  fn show(&mut self) {
    let now = SystemTime::now();
    let status = match (&self.session, &self.stopped) {
      (Some(session), _) => {
        let base = match session.until {
          Some(until) => format!("Awake: {} left", left(until, now)),
          None => "Awake".into(),
        };

        match &session.forced {
          Some(why) => format!("{base}, despite {why}"),
          None => base,
        }
      }
      (None, Some(why)) => format!("Off: {why}"),
      (None, None) => "Off".into(),
    };

    let forced = self.session.as_ref().is_some_and(|s| s.forced.is_some());
    let (symbols, description): (&[&'static str], _) = match &self.session {
      // Steam: on against a rule; the cup alone where the system lacks the symbol (macOS 13).
      Some(_) if forced => (&["cup.and.heat.waves.fill", "cup.and.saucer.fill"], status.clone()),
      Some(_) => (&["cup.and.saucer.fill"], INFO.name.into()),
      None => (&["cup.and.saucer"], format!("{}: off", INFO.name)),
    };

    if self.icon != symbols[0] {
      self.icon = symbols[0];
      self.tray.set_icon(appkit::symbol(symbols));
    }

    self.tray.set_tooltip(&description);
    // An open menu shows the new one once it closes: its status line and checkmarks change
    // meanwhile.
    let mode = self.session.as_ref().map(|s| s.mode);
    self.tray.set_menu(&menu(&status, mode));
    appkit::show_open(&status, (0..MODES.len()).map(|i| mode == Some(i)));

    if let Some(window) = &self.window {
      let shown = self.shown();
      window.with(|form| form.shown = shown);
    }
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
    return Some(format!("battery at {}%", battery.percent));
  }

  settings.off_unplugged.then(|| "running on battery".into())
}

/// The time from `now` until `until` as a clock counts down: `59:42`, `2:59:42`; whole seconds,
/// rounded up, so that it starts at the length chosen.
fn left(until: SystemTime, now: SystemTime) -> String {
  let left = until.duration_since(now).unwrap_or_default();
  let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
  match secs / 3600 {
    0 => format!("{}:{:02}", secs / 60, secs % 60),
    h => format!("{h}:{:02}:{:02}", secs / 60 % 60, secs % 60),
  }
}

// MARK: Launch

fn main() {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_writer(std::io::stderr)
    .init();

  tracing::info!(battery = ?power::battery(), low_power = power::low_power_mode(), "awake");
  nimbo::app(INFO, start);
}

fn start(main: &mut Main) {
  appkit::set_dock_icon();

  let tray = Tray::new(appkit::symbol(&["cup.and.saucer"]), &format!("{}: off", INFO.name));
  // The time left, the login item's state and the battery change while the menu is closed.
  tray.on_open(|_, _| with_state(State::show));
  tray.on_item(|main, _, action| with_state(|state| state.on_tray(main, action)));
  appkit::watch_menu(|open| with_state(|state| state.on_menu(open)));
  main.on_reopen(|main| with_state(|state| state.open_settings(main)));

  let state = State {
    tray,
    proxy: main.proxy(),
    window: None,
    settings: Settings::load(),
    session: None,
    stopped: None,
    timer: 0,
    menu_open: false,
    tick: 0,
    icon: "cup.and.saucer",
    _watch: None,
  };
  STATE.set(Some(state));

  let watch = power::watch(|change| with_state(|state| state.on_power(change)));
  with_state(|state| {
    state._watch = watch;
    if state.settings.on_launch {
      state.start(0, false);
    }

    state.show();
  });
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
    assert_eq!(INFO.id, RELEASE.id, "without AWAKE_FLAVOR, as `make test` builds, it is Awake");
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
  fn low_power_mode_ends_it_only_when_asked() {
    let asked = Settings { off_in_low_power: true, ..Settings::default() };
    assert_eq!(rule(&asked, true, None).as_deref(), Some("Low Power Mode"));
    assert_eq!(rule(&asked, false, None), None);
    assert_eq!(rule(&Settings::default(), true, None), None);
  }

  #[test]
  fn the_floor_ends_it_on_battery_only() {
    let floor = Settings { floor_on: true, floor: 20, ..Settings::default() };
    assert_eq!(rule(&floor, false, battery(20, true)).as_deref(), Some("battery at 20%"));
    assert_eq!(rule(&floor, false, battery(21, true)), None);
    assert_eq!(rule(&floor, false, battery(5, false)), None);
    assert_eq!(rule(&floor, false, None), None);
    assert_eq!(rule(&Settings { floor_on: false, ..floor }, false, battery(5, true)), None);
  }

  #[test]
  fn unplugging_ends_it_when_asked() {
    let asked = Settings { off_unplugged: true, ..Settings::default() };
    assert_eq!(rule(&asked, false, battery(90, true)).as_deref(), Some("running on battery"));
    assert_eq!(rule(&asked, false, battery(90, false)), None);
    assert_eq!(rule(&Settings::default(), false, battery(90, true)), None);
  }
}
