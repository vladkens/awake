// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The user's choices, in `NSUserDefaults` (the bundle's domain, or the executable's name when it
//! runs bare). What isn't stored is off, but the battery floor's level.

use objc2_foundation::{NSString, NSUserDefaults};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
  /// Awake as soon as the app starts.
  pub on_launch: bool,
  /// Only the system stays awake, the display sleeps as set (`caffeinate -i`).
  pub display_sleeps: bool,
  /// Locking the screen ends it.
  pub off_on_lock: bool,
  /// A timed session's time left beside the icon.
  pub show_left: bool,
  /// On battery below [`Settings::floor`] it ends.
  pub floor_on: bool,
  /// The charge, in percent: [`FLOOR_MIN`] to [`FLOOR_MAX`] by [`FLOOR_STEP`].
  pub floor: u32,
  /// Low Power Mode turning on ends it.
  pub off_in_low_power: bool,
  /// The power adapter unplugged ends it.
  pub off_unplugged: bool,
}

/// The battery floor's range and step, in percent: the slider's.
pub const FLOOR_MIN: u32 = 10;
pub const FLOOR_MAX: u32 = 90;
pub const FLOOR_STEP: u32 = 10;

impl Default for Settings {
  fn default() -> Self {
    Self {
      on_launch: false,
      display_sleeps: false,
      off_on_lock: false,
      show_left: false,
      floor_on: false,
      floor: 20,
      off_in_low_power: false,
      off_unplugged: false,
    }
  }
}

const ON_LAUNCH: &str = "activateOnLaunch";
const DISPLAY_SLEEPS: &str = "allowDisplaySleep";
const OFF_ON_LOCK: &str = "deactivateOnScreenLock";
const SHOW_LEFT: &str = "showTimeLeftInMenuBar";
const FLOOR_ON: &str = "batteryFloorEnabled";
const FLOOR: &str = "batteryFloor";
const OFF_IN_LOW_POWER: &str = "deactivateInLowPowerMode";
const OFF_UNPLUGGED: &str = "deactivateWhenUnplugged";
/// Sparkle's key for Skip This Version.
const SKIPPED: &str = "SUSkippedVersion";

impl Settings {
  pub fn load() -> Self {
    let defaults = NSUserDefaults::standardUserDefaults();
    let flag = |key| defaults.boolForKey(&NSString::from_str(key));
    let floor = defaults.integerForKey(&NSString::from_str(FLOOR));
    let floor = match u32::try_from(floor) {
      Ok(floor) if (FLOOR_MIN..=FLOOR_MAX).contains(&floor) && floor % FLOOR_STEP == 0 => floor,
      _ => Self::default().floor,
    };

    Self {
      on_launch: flag(ON_LAUNCH),
      display_sleeps: flag(DISPLAY_SLEEPS),
      off_on_lock: flag(OFF_ON_LOCK),
      show_left: flag(SHOW_LEFT),
      floor_on: flag(FLOOR_ON),
      floor,
      off_in_low_power: flag(OFF_IN_LOW_POWER),
      off_unplugged: flag(OFF_UNPLUGGED),
    }
  }

  pub fn save(&self) {
    let defaults = NSUserDefaults::standardUserDefaults();
    let flag = |key, on| defaults.setBool_forKey(on, &NSString::from_str(key));
    flag(ON_LAUNCH, self.on_launch);
    flag(DISPLAY_SLEEPS, self.display_sleeps);
    flag(OFF_ON_LOCK, self.off_on_lock);
    flag(SHOW_LEFT, self.show_left);
    flag(FLOOR_ON, self.floor_on);
    defaults.setInteger_forKey(self.floor as isize, &NSString::from_str(FLOOR));
    flag(OFF_IN_LOW_POWER, self.off_in_low_power);
    flag(OFF_UNPLUGGED, self.off_unplugged);
  }
}

/// The version Skip This Version passed over, which automatic checks don't offer.
pub fn skipped() -> Option<String> {
  let defaults = NSUserDefaults::standardUserDefaults();
  defaults.stringForKey(&NSString::from_str(SKIPPED)).map(|v| v.to_string())
}

pub fn skip(version: &str) {
  let defaults = NSUserDefaults::standardUserDefaults();
  let (version, key) = (NSString::from_str(version), NSString::from_str(SKIPPED));
  // SAFETY: a string is a property list's value.
  unsafe { defaults.setObject_forKey(Some(&version), &key) };
}
