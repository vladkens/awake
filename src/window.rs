// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The settings window, drawn by nimbo in the look of System Settings: toolbar tabs over pages of
//! grey plates. General has the rows of startup, the screen and the menu bar; Battery the rules
//! that turn it off, the floor's slider under its row; About the app, its updates, its links and
//! thanks. A row has a switch and a grey line under its name where the name doesn't say it all (an
//! ⓘ beside the name says why a row is off). The window is as high as the page shown. It shows what
//! the app gives it ([`Shown`]) and hands what the user changes to the app from the loop
//! ([`FormEvent`]), never while it draws. The app opens it when asked and drops it when closed.

use nimbo::autostart::State as Login;
use nimbo::{
  Align, App, Color, Config, ControlSize, Font, FormRow, FormStyle, Icon, Localized, Png, Slider,
  Style, TabItem, TextStyle, Ui, grow, px, rgb, t,
};

use crate::settings::{FLOOR_MAX, FLOOR_MIN, FLOOR_STEP, Settings};
use crate::updater::{self, State as Update};

const WIDTH: f32 = 480.0;
/// The slider's middle tick, labelled as its ends.
const FLOOR_MID: u32 = (FLOOR_MIN + FLOOR_MAX) / 2;
/// 80 px: 40 points at 2x; About's 96 points.
const ICON_LARGE: Png = Png::new(include_bytes!("../assets/icon.png")).side(192);
/// A battery three quarters full, as `battery.75percent`, in nimbo's 24-unit box.
const BATTERY: Icon = Icon::new(concat!(
  "M5 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z",
  "M21 10.5v3M6.5 10.5v3M9.5 10.5v3M12.5 10.5v3"
));

/// The rows' symbols, in nimbo's 24-unit box, each on a tile of its colour, as System Settings'
/// sidebar: `power`, `cup.and.saucer`, `display`, `lock`, `timer`, `battery.25percent`,
/// `powerplug`.
const POWER: Icon = Icon::new("M12 3.5v7.5M7.2 6.6a7.5 7.5 0 1 0 9.6 0");
const CUP: Icon = Icon::new(
  "M5 8.5h11v5a4.5 4.5 0 0 1-4.5 4.5h-2A4.5 4.5 0 0 1 5 13.5ZM16 10h1a2.5 2.5 0 0 1 0 5h-1.3M4 21h13",
);
const SCREEN: Icon = Icon::new(concat!(
  "M5 4.5h14a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 15V6A1.5 1.5 0 0 1 5 4.5Z",
  "M12 16.5v3.5M8.5 20h7"
));
const LOCK: Icon = Icon::new(concat!(
  "M7 10.5h10a1.5 1.5 0 0 1 1.5 1.5v7a1.5 1.5 0 0 1-1.5 1.5H7A1.5 1.5 0 0 1 5.5 19v-7A1.5 1.5 0 0 1 7 10.5Z",
  "M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5"
));
const TIMER: Icon =
  Icon::new("M12 8.5v4.5l2.5 1.5M10 2.5h4M4.5 13a7.5 7.5 0 1 0 15 0a7.5 7.5 0 1 0-15 0");
const LOW: Icon = Icon::new(concat!(
  "M5 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z",
  "M21 10.5v3M6.5 10.5v3"
));
const PLUG: Icon = Icon::new("M9 3v4.5M15 3v4.5M6.5 7.5h11V11a5.5 5.5 0 0 1-11 0ZM12 16.5V21");
const WHITE: Color = rgb(0xffffff);
const GREY: Color = rgb(0x8e8e93);
const ORANGE: Color = rgb(0xff9500);
const BLUE: Color = rgb(0x0a84ff);
const INDIGO: Color = rgb(0x5e5ce6);
const PURPLE: Color = rgb(0xbf5af2);
const GREEN: Color = rgb(0x30b94d);
const YELLOW: Color = rgb(0xf5b800);
const RED: Color = rgb(0xff453a);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Page {
  General,
  Battery,
  About,
}

fn pages() -> [TabItem<'static, Page>; 3] {
  [
    TabItem::new(Page::General, Icon::GEAR, t!("General").as_str()),
    TabItem::new(Page::Battery, BATTERY, t!("Battery").as_str()),
    TabItem::new(Page::About, Icon::INFO, t!("About").as_str()),
  ]
}

const SOURCE: &str = env!("CARGO_PKG_REPOSITORY");
const COFFEE: &str = "https://buymeacoffee.com/vladkens";

/// `(name, what, url)`.
const ACKS: [(&str, Localized, &str); 2] = [
  ("nimbo", t!("The UI toolkit Awake is drawn with"), "https://github.com/vladkens/nimbo"),
  (
    "KeepingYouAwake",
    t!("The app that inspired Awake"),
    "https://github.com/newmarcel/KeepingYouAwake",
  ),
];

const DISPLAY: Localized = t!("Only the Mac stays awake.");
const LOCKS: Localized = t!("Locking the screen ends Keep Awake.");
const SHOW_LEFT: Localized = t!("Beside the cup, while a timed session runs.");
const LOGIN: Localized = t!("Awake opens when you log in.");
const ON_LAUNCH: Localized = t!("Keep Awake turns on as Awake opens.");
const FLOOR: Localized = t!("On battery. Turned on by hand below it, it stays on.");
const LOW_POWER: Localized = t!("Set in System Settings → Battery.");
const UNPLUGGED: Localized = t!("Keep Awake runs only on the power adapter.");
const APPROVE: Localized = t!("Allow it in System Settings → Login Items.");
const BUNDLE: Localized = t!("Works only from Awake.app.");
const NO_BATTERY: Localized = t!("This Mac has no battery.");

/// What the window shows besides the settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
  pub settings: Settings,
  /// The Mac has a battery.
  pub battery: bool,
  /// Launch at login; `Err` where it can't (a bare executable).
  pub login: Result<Login, String>,
  /// The updater's state, `None` where the app can't update.
  pub update: Option<Update>,
}

/// What the user changed in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormEvent {
  Changed(Settings),
  Login(bool),
  /// Check for Updates.
  Check,
  /// Install the update on offer, or try a failed one again.
  Install,
}

pub struct Form {
  page: Page,
  pub shown: Shown,
  /// Hands an event to the app, which takes it from the loop.
  on: Box<dyn Fn(FormEvent)>,
}

impl Form {
  /// The window's config and its app; `on` hears the [`FormEvent`]s and takes each from the loop.
  pub fn new(shown: Shown, on: impl Fn(FormEvent) + 'static) -> (Config, Self) {
    // The first frame fits the height to the page ([`Ui::fit_window_height`]).
    let cfg = Config::new(crate::INFO.name, WIDTH, 400.0).fixed().unified();
    let form = Self { page: Page::General, shown, on: Box::new(on) };
    (cfg.localizations(crate::LOCALES), form)
  }

  fn send(&self, event: FormEvent) {
    (self.on)(event);
  }

  fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, t!("Startup"), |ui| {
      let (mut login, why) = match &self.shown.login {
        Ok(state) => {
          (state.wanted(), (*state == Login::RequiresApproval).then_some(APPROVE.as_str()))
        }
        Err(_) => (false, Some(BUNDLE.as_str())),
      };

      let row = FormRow::new(label(t!("Launch at login").as_str(), why)).caption(LOGIN).tile(POWER);
      ui.disabled(self.shown.login.is_err()).form_row(&tiled(fs, GREY), row, |ui| {
        if ui.toggle(&mut login, "").changed {
          self.send(FormEvent::Login(login));
        }
      });

      let s = &mut self.shown.settings;
      let row = FormRow::new(t!("Keep awake at launch")).caption(ON_LAUNCH).tile(CUP);
      ui.form_row(&tiled(fs, ORANGE), row, |ui| ui.toggle(&mut s.on_launch, ""));
    });

    let s = &mut self.shown.settings;
    ui.form_section(fs, t!("Screen"), |ui| {
      let row = FormRow::new(t!("Let the display sleep")).caption(DISPLAY).tile(SCREEN);
      ui.form_row(&tiled(fs, BLUE), row, |ui| ui.toggle(&mut s.display_sleeps, ""));

      let row = FormRow::new(t!("Turn off when the screen locks")).caption(LOCKS).tile(LOCK);
      ui.form_row(&tiled(fs, INDIGO), row, |ui| ui.toggle(&mut s.off_on_lock, ""));
    });

    ui.form_section(fs, t!("Menu Bar"), |ui| {
      let row = FormRow::new(t!("Show the time left")).caption(SHOW_LEFT).tile(TIMER);
      ui.form_row(&tiled(fs, PURPLE), row, |ui| ui.toggle(&mut s.show_left, ""));
    });
  }

  fn battery(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let s = &mut self.shown.settings;
    // With no battery the rows that need one are off, their ⓘ says why.
    let no_battery = !self.shown.battery;
    let why = no_battery.then_some(NO_BATTERY.as_str());

    // Translators: the header of the rows Below 20% battery, In Low Power Mode, When unplugged.
    ui.form_section(fs, t!("Turn Off Keep Awake"), |ui| {
      let name = t!("Below {percent}% battery", percent = s.floor);
      let (off, floor) = (!s.floor_on, &mut s.floor);
      let level = |ui: &mut Ui, _: TextStyle| {
        let mut v = *floor as f32;
        let slider = Slider::new(&mut v, FLOOR_MIN as f32..=FLOOR_MAX as f32)
          .step(FLOOR_STEP as f32)
          .tick_label(|v| {
            let percent = v as u32;
            // Translators: the battery slider's ticks: 10%, 50%, 90%.
            [FLOOR_MIN, FLOOR_MID, FLOOR_MAX].contains(&percent).then(|| t!("{percent}%", percent))
          });

        if slider.show(ui.disabled(off).a11y_label(&t!("Battery level"))).changed {
          // The slider snaps to its steps.
          *floor = v.round() as u32;
        }
      };
      let row = FormRow::new(label(&name, why)).caption(FLOOR).footer(level).tile(LOW);
      ui.disabled(no_battery).form_row(&tiled(fs, RED), row, |ui| ui.toggle(&mut s.floor_on, ""));

      let row = FormRow::new(t!("In Low Power Mode")).caption(LOW_POWER).tile(BATTERY);
      ui.form_row(&tiled(fs, YELLOW), row, |ui| ui.toggle(&mut s.off_in_low_power, ""));

      let row =
        FormRow::new(label(t!("When unplugged").as_str(), why)).caption(UNPLUGGED).tile(PLUG);
      ui.disabled(no_battery)
        .form_row(&tiled(fs, GREEN), row, |ui| ui.toggle(&mut s.off_unplugged, ""));
    });
  }

  /// The app's icon, name and lines, centred, its links and its acknowledgements.
  fn about(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let t = ui.theme();
    let column = Style::new().w(grow()).gap(4.0).align(Align::Center, Align::Start);
    ui.col(column, |ui| {
      ui.image(ICON_LARGE, Style::new().w(px(96.0)).h(px(96.0)));
      ui.el(Style::new().h(px(6.0)));
      ui.text(crate::INFO.name, t.label().font(Font { size: 17.0, ..t.bold }));
      ui.text(t!("Version {version}", version = crate::VERSION), fs.caption);
      ui.text(t!("Keeps your Mac awake from the menu bar."), fs.caption);
      ui.el(Style::new().h(px(6.0)));

      let (text, button) = self.shown.update.clone().map(update).unwrap_or_default();
      if let Some(text) = text {
        ui.text(&text, fs.caption);
        ui.el(Style::new().h(px(6.0)));
      }

      ui.row(Style::new().gap(8.0), |ui| {
        let tip = |url: &'static str| url.trim_start_matches("https://");
        if ui.id(("link", "source")).help(tip(SOURCE)).button("GitHub").clicked {
          open(SOURCE);
        }

        if ui.id(("link", "coffee")).help(tip(COFFEE)).button(&t!("Donate")).clicked {
          open(COFFEE);
        }

        if let Some((label, event)) = button
          && ui.id("update").button(&label).clicked
        {
          self.send(event);
        }
      });
    });

    // A row's controls go on its name's line: the ↗ is centred in the whole row instead.
    let row_h = 2.0 * fs.pad_y + ui.line_height(fs.label) + 2.0 + ui.line_height(fs.caption);
    ui.form_section(fs, t!("Acknowledgements"), |ui| {
      for (name, what, url) in ACKS {
        ui.form_row(fs, FormRow::new(name).caption(what), |ui| {
          ui.col(Style::new().h(px(row_h)).align(Align::Center, Align::Center), |ui| {
            let tip = url.trim_start_matches("https://");
            let button = ui.id(("link", name)).control_size(ControlSize::Large).help(tip);
            if button.icon_button(Icon::EXTERNAL).clicked {
              open(url);
            }
          });
        });
      }
    });
  }
}

/// The updater's line and its button, as Sparkle's About: Check for Updates, what the check found,
/// the download.
fn update(update: Update) -> (Option<String>, Option<(Localized, FormEvent)>) {
  let name = crate::INFO.name;
  let check = Some((t!("Check for Updates"), FormEvent::Check));
  match update {
    Update::Idle => (None, check),
    Update::Checking => (Some(t!("Checking for updates…").to_string()), None),
    Update::UpToDate => (Some(t!("{name} is up to date.", name)), check),
    Update::Available(r) => {
      let text = t!("{name} {version} is available.", name, version = updater::shown(&r.version));
      (Some(text), Some((t!("Install and Relaunch"), FormEvent::Install)))
    }
    Update::Downloading(_, done) => (Some(t!("Downloading the update… {done}%", done)), None),
    Update::Failed(r, why) => {
      let (text, again) = match r {
        Some(_) => (t!("The update failed: {why}", why), FormEvent::Install),
        None => (t!("Couldn't check for updates: {why}", why), FormEvent::Check),
      };

      (Some(text), Some((t!("Try Again"), again)))
    }
    // The check at launch offers the release again, unless it can't update any more.
    Update::Relaunched(why) => (Some(t!("The update failed: {why}", why)), None),
  }
}

/// `fs` with its rows' tiles in `color`, their symbols white.
fn tiled(fs: &FormStyle, color: Color) -> FormStyle {
  FormStyle { tile: fs.tile.bg(color), tile_text: fs.tile_text.color(WHITE), ..*fs }
}

/// `url` in the browser.
fn open(url: &str) {
  if let Err(e) = nimbo::open::url(url) {
    tracing::warn!("open {url}: {e}");
  }
}

/// A row's name, with an ⓘ that says why the row is off when it is.
fn label<'s>(name: &'s str, why: Option<&'s str>) -> impl FnOnce(&mut Ui, TextStyle) + 's {
  move |ui: &mut Ui, s: TextStyle| {
    ui.text(name, s);
    // Keyed: on a row's name nimbo boxes an ⓘ in an element of the ⓘ's own position id.
    if let Some(why) = why {
      ui.id(("why", name)).help(why).info_button();
    }
  }
}

impl App for Form {
  fn view(&mut self, ui: &mut Ui) {
    ui.fit_window_height(WIDTH, 200.0, 800.0);
    let before = self.shown.settings;
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &pages());
      // Keyed by the tab: each page's switches keep their own state.
      ui.id(("page", self.page)).col(fs.page, |ui| match self.page {
        Page::General => self.general(ui, &fs),
        Page::Battery => self.battery(ui, &fs),
        Page::About => self.about(ui, &fs),
      });
    });

    if self.shown.settings != before {
      self.send(FormEvent::Changed(self.shown.settings));
    }
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;
  use std::time::Duration;

  use nimbo::{Appearance, Event, Harness, Locale, harness};
  use shipo::Release;

  use super::*;

  /// The window with a battery and launch at login on, in the language of `AWAKE_SHOTS_LOCALE`
  /// (`make shots LOCALE=…`), English without it.
  fn window(update: Option<Update>) -> Harness<Form> {
    let settings = Settings { floor_on: true, ..Settings::default() };
    let shown = Shown { settings, battery: true, login: Ok(Login::Enabled), update };
    let (cfg, form) = Form::new(shown, |_| {});
    let mut h = Harness::new(form, cfg.size, 2.0);
    h.set_titlebar(Some(harness::mac_titlebar(cfg.title, false)));
    if let Ok(tag) = std::env::var("AWAKE_SHOTS_LOCALE") {
      h.set_locale(Locale::Tag(tag.leak()), crate::LOCALES);
    }

    h
  }

  /// Every page in both looks, and About in each state of the updater, as wide as the window and
  /// no higher than it fits; with `AWAKE_SHOTS` set (`make shots`), written there.
  #[test]
  fn every_page_renders() {
    let dir = std::env::var_os("AWAKE_SHOTS").map(PathBuf::from);
    let shot = |h: &mut Harness<Form>, name: &str| {
      h.wait(Duration::from_millis(400));
      assert_eq!(h.size().w, WIDTH, "{name}");
      assert!((200.0..800.0).contains(&h.size().h), "{name}: {:?}", h.size());
      if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
        h.shot(&dir.join(format!("{name}.png"))).unwrap();
      }
    };

    for (look, appearance) in [("light", Appearance::LIGHT), ("dark", Appearance::DARK)] {
      let mut h = window(Some(Update::Idle));
      h.event(Event::Appearance(appearance));
      for page in [Page::General, Page::Battery, Page::About] {
        h.app_mut().page = page;
        shot(&mut h, &format!("{page:?}-{look}").to_lowercase());
      }
    }

    let release = Release::test("1.2.0");
    let why = "The Internet connection appears to be offline.".to_string();
    let states = [
      ("checking", Update::Checking),
      ("up-to-date", Update::UpToDate),
      ("available", Update::Available(release.clone())),
      ("downloading", Update::Downloading(release.clone(), 42)),
      ("install-failed", Update::Failed(Some(release), why.clone())),
      ("check-failed", Update::Failed(None, why.clone())),
      ("relaunched", Update::Relaunched(why)),
    ];
    for (name, update) in states {
      let mut h = window(Some(update));
      h.event(Event::Appearance(Appearance::LIGHT));
      h.app_mut().page = Page::About;
      shot(&mut h, &format!("about-{name}"));
    }
  }

  /// The window speaks the language it is given: the tabs, the rows, the number in a row's name.
  #[test]
  fn the_window_speaks_russian() {
    let mut h = window(None);
    h.set_locale(Locale::Tag("ru"), crate::LOCALES);
    h.app_mut().page = Page::Battery;
    h.wait(Duration::from_millis(100));
    let a11y = h.a11y();
    // The tree's texts are escaped: a no-break space before the percent sign, as Russian has it.
    for text in ["Основные", "Аккумулятор", r"Заряд ниже 20\u{a0}%", "В режиме энергосбережения"]
    {
      assert!(a11y.contains(text), "{text} in {a11y}");
    }
  }
}
