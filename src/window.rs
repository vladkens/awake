// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The settings window, drawn by nimbo in the look of System Settings: toolbar tabs over grey
//! plates of rows, each a setting with a grey note under its name and a switch, and the battery
//! floor's slider; one height for every tab, the tallest's. It shows what the app gives it
//! ([`Shown`]) and hands what the user changes to the app from the loop ([`FormEvent`]), never
//! while it draws. The app opens it when asked and drops it when closed.

use nimbo::autostart::State as Login;
use nimbo::{
  Align, App, Config, Font, FormRow, FormStyle, Icon, Png, Proxy, Section, Style, TabItem,
  TextStyle, Ui, grow, px,
};

use crate::settings::{FLOOR_MAX, FLOOR_MIN, FLOOR_STEP, Settings};

const WIDTH: f32 = 480.0;
/// The Battery tab's height, the tallest, as nimbo lays it out: switching tabs neither resizes
/// nor moves the window.
const HEIGHT: f32 = 466.0;
const ICON: Png = Png::new(include_bytes!("../assets/icon.png")).side(192);
/// A battery three quarters full, as `battery.75percent`, in nimbo's 24-unit box.
const BATTERY: Icon = Icon::new(concat!(
  "M5 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z",
  "M21 10.5v3M6.5 10.5v3M9.5 10.5v3M12.5 10.5v3"
));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
  General,
  Battery,
  About,
}

const PAGES: [TabItem<Page>; 3] = [
  TabItem::new(Page::General, Icon::GEAR, "General"),
  TabItem::new(Page::Battery, BATTERY, "Battery"),
  TabItem::new(Page::About, Icon::INFO, "About"),
];

/// The texts that name the app say `{app}`, [`named`]: Awake Dev's own in its flavor.
const LOGIN: &str = "Opens {app} when you log in to your Mac.";
const ON_LAUNCH: &str = "Starts keeping the Mac awake as soon as {app} opens.";
const DISPLAY: &str = "Only the Mac stays awake; the screen turns off as set in System Settings.";
const FLOOR: &str =
  "Only on battery. Turned on by hand below this level, {app} stays on until you plug in.";
const LOW_POWER: &str = "Low Power Mode is in System Settings → Battery.";
const UNPLUGGED: &str = "{app} keeps the Mac awake only while it runs on its adapter.";
const APPROVE: &str = "Allow {app} in System Settings → Login Items";
const BUNDLE: &str = "Only Awake.app can launch at login: make install";
const NO_BATTERY: &str = "This Mac has no battery";

/// What the window shows besides the settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
  pub settings: Settings,
  /// The Mac has a battery.
  pub battery: bool,
  /// Launch at login; `Err` where it can't (a bare executable).
  pub login: Result<Login, String>,
}

/// What the user changed in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormEvent {
  Changed(Settings),
  Login(bool),
}

pub struct Form {
  page: Page,
  pub shown: Shown,
  proxy: Proxy,
  on: fn(FormEvent),
}

impl Form {
  /// The window's config and its app; `on` hears the [`FormEvent`]s, from the loop.
  pub fn new(shown: Shown, proxy: Proxy, on: fn(FormEvent)) -> (Config, Self) {
    let cfg = Config::new(crate::INFO.name, WIDTH, HEIGHT).fixed().unified();
    (cfg, Self { page: Page::General, shown, proxy, on })
  }

  fn send(&self, event: FormEvent) {
    let on = self.on;
    self.proxy.post(move |_| on(event));
  }

  fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
    ui.form_section(fs, Section::default(), |ui| {
      let (mut login, tip) = match &self.shown.login {
        Ok(state) => (state.wanted(), (*state == Login::RequiresApproval).then(|| named(APPROVE))),
        Err(_) => (false, Some(BUNDLE.into())),
      };

      let caption = named(LOGIN);
      let row = FormRow::new(label("Launch at login", tip.as_deref())).caption(&caption);
      ui.disabled(self.shown.login.is_err()).form_row(fs, row, |ui| {
        if ui.toggle(&mut login, "").changed {
          self.send(FormEvent::Login(login));
        }
      });

      let s = &mut self.shown.settings;
      let caption = named(ON_LAUNCH);
      let row = FormRow::new("Keep awake on launch").caption(&caption);
      ui.form_row(fs, row, |ui| ui.toggle(&mut s.on_launch, ""));
      let row = FormRow::new("Allow display to sleep").caption(DISPLAY);
      ui.form_row(fs, row, |ui| ui.toggle(&mut s.display_sleeps, ""));
    });
  }

  fn battery(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let s = &mut self.shown.settings;
    let tip = (!self.shown.battery).then_some(NO_BATTERY);
    ui.form_section(fs, "Battery Level", |ui| {
      let name = format!("Turn off when battery is below {}%", s.floor);
      let caption = named(FLOOR);
      let row = FormRow::new(label(&name, tip)).caption(&caption);
      ui.disabled(tip.is_some()).form_row(fs, row, |ui| ui.toggle(&mut s.floor_on, ""));
      let mut floor = s.floor as f32;
      ui.disabled(tip.is_some() || !s.floor_on).form_row(fs, FormRow::new("Level"), |ui| {
        let t = ui.theme().label();
        ui.text(format!("{FLOOR_MIN}%"), t);
        if ui.slider(&mut floor, FLOOR_MIN as f32..=FLOOR_MAX as f32).changed {
          s.floor = (floor / FLOOR_STEP as f32).round() as u32 * FLOOR_STEP;
        }

        ui.text(format!("{FLOOR_MAX}%"), t);
      });
    });
    ui.form_section(fs, "Low Power Mode", |ui| {
      let row = FormRow::new("Turn off in Low Power Mode").caption(LOW_POWER);
      ui.form_row(fs, row, |ui| ui.toggle(&mut s.off_in_low_power, ""));
    });
    ui.form_section(fs, "Power Adapter", |ui| {
      let caption = named(UNPLUGGED);
      let row = FormRow::new(label("Turn off when unplugged", tip)).caption(&caption);
      ui.disabled(tip.is_some()).form_row(fs, row, |ui| ui.toggle(&mut s.off_unplugged, ""));
    });
  }

  /// The app's icon, name and lines, centred.
  fn about(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let t = ui.theme();
    let column = Style::new().w(grow()).gap(4.0).align(Align::Center, Align::Start);
    ui.col(column, |ui| {
      ui.png_image(ICON, Style::new().w(px(96.0)).h(px(96.0)));
      ui.el(Style::new().h(px(6.0)));
      ui.text(crate::INFO.name, t.label().font(Font { size: 17.0, ..t.bold }));
      ui.text(format!("Version {}", env!("CARGO_PKG_VERSION")), fs.caption);
      ui.text("Keeps your Mac awake from the menu bar.", fs.caption);
      ui.text("© 2026 vladkens · MIT License", fs.caption);
    });
  }
}

/// `text` with the app's name for `{app}`.
fn named(text: &str) -> String {
  text.replace("{app}", crate::INFO.name)
}

/// A row's name, with an ⓘ that says why it is off when it is.
fn label<'s>(name: &'s str, tip: Option<&'s str>) -> impl FnOnce(&mut Ui, TextStyle) + 's {
  move |ui: &mut Ui, s: TextStyle| {
    ui.text(name, s);
    if let Some(tip) = tip {
      ui.help(tip).info_button();
    }
  }
}

impl App for Form {
  fn view(&mut self, ui: &mut Ui) {
    let before = self.shown.settings;
    let fs = FormStyle::plates(&ui.theme());
    ui.col(Style::new().grow().bg(fs.bg), |ui| {
      ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &PAGES);
      ui.col(fs.page, |ui| match self.page {
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
