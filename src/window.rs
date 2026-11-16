// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The settings window, drawn by nimbo in the look of System Settings: toolbar tabs over pages of
//! grey plates. General has Keep Awake's switch beside the app's icon first, then the rows of
//! startup and the screen; Battery the rules that turn it off, the floor's slider
//! under its row; About the app, its links and thanks. A row has a switch and a grey line under
//! its name where the name doesn't say it all (an ⓘ beside the name says why a row is off). The
//! window is as high as the page shown. It shows what the app gives it ([`Shown`]) and hands what
//! the user changes to the app from the loop ([`FormEvent`]), never while it draws. The app opens
//! it when asked and drops it when closed.

use nimbo::autostart::State as Login;
use nimbo::{
  Align, App, Color, Config, ControlSize, Font, FormRow, FormStyle, Icon, Png, Proxy, Section,
  Slider, Style, TabItem, TextStyle, Ui, grow, px, rgb,
};

use crate::settings::{FLOOR_MAX, FLOOR_MIN, FLOOR_STEP, Settings};

const WIDTH: f32 = 480.0;
/// The slider's middle tick, labelled as its ends.
const FLOOR_MID: u32 = (FLOOR_MIN + FLOOR_MAX) / 2;
/// 80 px: 40 points at 2x; About's 96 points.
const ICON: Png = Png::new(include_bytes!("../assets/icon.png")).side(80);
const ICON_LARGE: Png = Png::new(include_bytes!("../assets/icon.png")).side(192);
/// A battery three quarters full, as `battery.75percent`, in nimbo's 24-unit box.
const BATTERY: Icon = Icon::new(concat!(
  "M5 7h11a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z",
  "M21 10.5v3M6.5 10.5v3M9.5 10.5v3M12.5 10.5v3"
));

/// The rows' symbols, in nimbo's 24-unit box, each on a tile of its colour, as System Settings'
/// sidebar: `power`, `cup.and.saucer`, `display`, `lock`, `battery.25percent`,
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
const GREEN: Color = rgb(0x30b94d);
const YELLOW: Color = rgb(0xf5b800);
const RED: Color = rgb(0xff453a);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

const SOURCE: &str = env!("CARGO_PKG_REPOSITORY");
const COFFEE: &str = "https://buymeacoffee.com/vladkens";

/// `(name, what, url)`.
const ACKS: [(&str, &str, &str); 2] = [
  ("nimbo", "The UI toolkit Awake is drawn with", "https://github.com/vladkens/nimbo"),
  (
    "KeepingYouAwake",
    "The app that inspired Awake",
    "https://github.com/newmarcel/KeepingYouAwake",
  ),
];

const DISPLAY: &str = "Only the Mac stays awake.";
const LOCKS: &str = "Locking the screen ends Keep Awake.";
const FLOOR: &str = "Only on battery.";
const APPROVE: &str = "Allow it in System Settings → Login Items.";
const BUNDLE: &str = "Works only from the installed app (make install).";
const NO_BATTERY: &str = "This Mac has no battery.";

/// What the window shows besides the settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
  pub settings: Settings,
  /// Awake now, and until when or why not, as its first row says it.
  pub awake: bool,
  pub status: String,
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
  /// Keep Awake turned on, until turned off, or off.
  Awake(bool),
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
    // The first frame fits the height to the page ([`Ui::fit_window_height`]).
    let cfg = Config::new(crate::INFO.name, WIDTH, 400.0).fixed().unified();
    (cfg, Self { page: Page::General, shown, proxy, on })
  }

  fn send(&self, event: FormEvent) {
    let on = self.on;
    self.proxy.post(move |_| on(event));
  }

  fn general(&mut self, ui: &mut Ui, fs: &FormStyle) {
    self.head(ui, fs);

    ui.form_section(fs, "Startup", |ui| {
      let (mut login, why) = match &self.shown.login {
        Ok(state) => (state.wanted(), (*state == Login::RequiresApproval).then_some(APPROVE)),
        Err(_) => (false, Some(BUNDLE)),
      };

      let row = FormRow::new(label("Launch at login", why)).tile(POWER);
      ui.disabled(self.shown.login.is_err()).form_row(&tiled(fs, GREY), row, |ui| {
        if ui.toggle(&mut login, "").changed {
          self.send(FormEvent::Login(login));
        }
      });

      let s = &mut self.shown.settings;
      let row = FormRow::new("Keep awake at launch").tile(CUP);
      ui.form_row(&tiled(fs, ORANGE), row, |ui| ui.toggle(&mut s.on_launch, ""));
    });

    let s = &mut self.shown.settings;
    ui.form_section(fs, "Screen", |ui| {
      let row = FormRow::new("Let the display sleep").caption(DISPLAY).tile(SCREEN);
      ui.form_row(&tiled(fs, BLUE), row, |ui| ui.toggle(&mut s.display_sleeps, ""));

      let row = FormRow::new("Turn off when the screen locks").caption(LOCKS).tile(LOCK);
      ui.form_row(&tiled(fs, INDIGO), row, |ui| ui.toggle(&mut s.off_on_lock, ""));
    });
  }

  /// The app's icon over Keep Awake's switch, as a System Settings pane's head.
  fn head(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let fs = &FormStyle { tile: Style::new().w(px(40.0)).h(px(40.0)), pad_y: 10.0, ..*fs };
    ui.form_section(fs, Section::default(), |ui| {
      let icon = |ui: &mut Ui, _: TextStyle| ui.image(ICON, Style::new().w(px(40.0)).h(px(40.0)));
      let row = FormRow::new("Keep Awake").caption(&self.shown.status).tile(icon);
      let mut awake = self.shown.awake;
      ui.form_row(fs, row, |ui| {
        if ui.toggle(&mut awake, "").changed {
          self.send(FormEvent::Awake(awake));
        }
      });
    });
  }

  fn battery(&mut self, ui: &mut Ui, fs: &FormStyle) {
    let s = &mut self.shown.settings;
    // With no battery the rows that need one are off, their ⓘ says why.
    let no_battery = !self.shown.battery;
    let why = no_battery.then_some(NO_BATTERY);

    ui.form_section(fs, "Turn Off Keep Awake", |ui| {
      let name = format!("Below {}% battery", s.floor);
      let (off, floor) = (!s.floor_on, &mut s.floor);
      let level = |ui: &mut Ui, _: TextStyle| {
        let mut v = *floor as f32;
        let slider = Slider::new(&mut v, FLOOR_MIN as f32..=FLOOR_MAX as f32)
          .step(FLOOR_STEP as f32)
          .tick_label(|v| {
            [FLOOR_MIN, FLOOR_MID, FLOOR_MAX].contains(&(v as u32)).then(|| format!("{v}%"))
          });

        if slider.show(ui.disabled(off).a11y_label("Battery level")).changed {
          // The slider snaps to its steps.
          *floor = v.round() as u32;
        }
      };
      let row = FormRow::new(label(&name, why)).caption(FLOOR).footer(level).tile(LOW);
      ui.disabled(no_battery).form_row(&tiled(fs, RED), row, |ui| ui.toggle(&mut s.floor_on, ""));

      let row = FormRow::new("In Low Power Mode").tile(BATTERY);
      ui.form_row(&tiled(fs, YELLOW), row, |ui| ui.toggle(&mut s.off_in_low_power, ""));

      let row = FormRow::new(label("When unplugged", why)).tile(PLUG);
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
      ui.text(concat!("Version ", env!("CARGO_PKG_VERSION")), fs.caption);
      ui.text("Keeps your Mac awake from the menu bar.", fs.caption);
      ui.el(Style::new().h(px(6.0)));
      ui.row(Style::new().gap(8.0), |ui| {
        if ui.id(("link", "source")).button("View on GitHub").clicked {
          open(SOURCE);
        }

        if ui.id(("link", "coffee")).button("Buy Me a Coffee").clicked {
          open(COFFEE);
        }
      });
    });

    // A row's controls go on its name's line: the ↗ is centred in the whole row instead.
    let row_h = 2.0 * fs.pad_y + ui.line_height(fs.label) + 2.0 + ui.line_height(fs.caption);
    ui.form_section(fs, "Acknowledgements", |ui| {
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
      ui.id("tabs").toolbar_tabs(&fs, &mut self.page, &PAGES);
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
