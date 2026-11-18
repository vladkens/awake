// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! What keeps the Mac awake (a power assertion, as `caffeinate` takes one) and what the app watches
//! to let it sleep again: the battery, Low Power Mode, the screen locking and the clock. All come
//! as notifications on the main run loop: nothing polls.

use std::cell::OnceCell;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
use objc2_core_foundation::{
  CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
  CFRetained, CFRunLoop, CFRunLoopSource, CFString, CFType, kCFRunLoopCommonModes,
};
use objc2_foundation::{
  NSDictionary, NSNotification, NSNotificationCenter, NSNotificationName, NSNumber,
  NSOperationQueue, NSProcessInfo, NSProcessInfoPowerStateDidChangeNotification, NSString,
  NSSystemClockDidChangeNotification,
};
use objc2_io_kit::{
  IOPMAssertionCreateWithName, IOPMAssertionID, IOPMAssertionRelease, IOPSCopyPowerSourcesInfo,
  IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription, IOPSGetProvidingPowerSourceType,
  IOPSNotificationCreateRunLoopSource, kIOPMAssertionLevelOn, kIOPSBatteryPowerValue,
  kIOPSCurrentCapacityKey, kIOPSInternalBatteryType, kIOPSMaxCapacityKey, kIOPSTypeKey,
  kIOReturnSuccess,
};

/// The Mac stays awake while this lives: the display too (`caffeinate -d`), or only the system
/// (`caffeinate -i`), the display sleeping as set. The system drops it with the process.
pub struct Assertion(IOPMAssertionID);

impl Assertion {
  pub fn new(display: bool) -> Option<Self> {
    // `kIOPMAssertPreventUserIdleDisplaySleep` and `…SystemSleep`, macros of IOPMLib.h.
    let kind = if display { "PreventUserIdleDisplaySleep" } else { "PreventUserIdleSystemSleep" };
    let kind = CFString::from_static_str(kind);
    let name = CFString::from_str(&format!("{} keeps the Mac awake", crate::INFO.name));
    let mut id = 0;
    // SAFETY: both strings are there, `id` is ours to write.
    let ret = unsafe {
      IOPMAssertionCreateWithName(Some(&kind), kIOPMAssertionLevelOn, Some(&name), &mut id)
    };
    if ret != kIOReturnSuccess {
      tracing::warn!("power assertion {kind}: error {ret:#x}");
      return None;
    }

    Some(Self(id))
  }
}

impl Drop for Assertion {
  fn drop(&mut self) {
    IOPMAssertionRelease(self.0);
  }
}

/// The internal battery's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
  /// Charge, 0–100.
  pub percent: u32,
  /// The Mac runs on it (not on its adapter).
  pub draining: bool,
}

/// The internal battery, if the Mac has one.
pub fn battery() -> Option<Battery> {
  let blob = IOPSCopyPowerSourcesInfo()?;
  // SAFETY: a blob of `IOPSCopyPowerSourcesInfo`.
  let sources = unsafe { IOPSCopyPowerSourcesList(Some(&blob)) }?;
  // SAFETY: as above.
  let providing = unsafe { IOPSGetProvidingPowerSourceType(Some(&blob)) };
  let draining = providing.is_some_and(|p| p.to_string() == text(kIOPSBatteryPowerValue));

  (0..sources.count()).find_map(|i| {
    // SAFETY: in bounds; the list's values are power sources (CFTypeRef).
    let source = unsafe { (sources.value_at_index(i) as *const CFType).as_ref() }?;
    // SAFETY: the blob and one of its sources.
    let info = unsafe { IOPSGetPowerSourceDescription(Some(&blob), Some(source)) }?;
    let info = bridge(&info);
    if string(info, kIOPSTypeKey)? != text(kIOPSInternalBatteryType) {
      return None;
    }

    let (now, max) = (number(info, kIOPSCurrentCapacityKey)?, number(info, kIOPSMaxCapacityKey)?);
    let percent = if max > 0 { (now * 100 / max).min(100) as u32 } else { 0 };
    Some(Battery { percent, draining })
  })
}

fn text(key: &CStr) -> &str {
  key.to_str().unwrap_or_default()
}

/// The dictionary as Foundation's: toll-free bridged.
fn bridge(dict: &CFDictionary) -> &NSDictionary<NSString, AnyObject> {
  // SAFETY: CFDictionary and NSDictionary are toll-free bridged; a power source's description has
  // string keys.
  unsafe { &*(dict as *const CFDictionary).cast() }
}

fn string(info: &NSDictionary<NSString, AnyObject>, key: &CStr) -> Option<String> {
  let value = info.objectForKey(&NSString::from_str(text(key)))?;
  Some(value.downcast_ref::<NSString>()?.to_string())
}

fn number(info: &NSDictionary<NSString, AnyObject>, key: &CStr) -> Option<i64> {
  let value = info.objectForKey(&NSString::from_str(text(key)))?;
  Some(value.downcast_ref::<NSNumber>()?.as_i64())
}

pub fn low_power_mode() -> bool {
  NSProcessInfo::processInfo().isLowPowerModeEnabled()
}

/// What the app hears about power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
  /// The battery's charge or the power source changed.
  Battery,
  /// Low Power Mode turned on or off.
  LowPowerMode,
  /// The Mac woke from sleep.
  Woke,
  /// The clock was set: the ends, wall clock times, come sooner or later.
  Clock,
  /// The screen locked.
  Locked,
}

/// A notification center and the token of what the app hears from it.
type Observer = (Retained<NSNotificationCenter>, Retained<ProtocolObject<dyn NSObjectProtocol>>);

/// The app's subscriptions; dropping it ends them.
pub struct Watch {
  source: CFRetained<CFRunLoopSource>,
  observers: Vec<Observer>,
}

thread_local! {
  static ON: OnceCell<fn(Change)> = const { OnceCell::new() };
}

/// The lock's observer in the distributed center, by its address.
static LOCK: u8 = 0;

/// Calls `on` on the main thread with every [`Change`]; the first call wins.
pub fn watch(on: fn(Change)) -> Option<Watch> {
  ON.with(|cell| cell.set(on).ok())?;

  unsafe extern "C-unwind" fn changed(_context: *mut c_void) {
    if let Some(on) = ON.with(|cell| cell.get().copied()) {
      on(Change::Battery);
    }
  }

  // SAFETY: the callback takes no context.
  let source = unsafe { IOPSNotificationCreateRunLoopSource(Some(changed), std::ptr::null_mut()) }?;
  // Common modes: also while the menu is open.
  // SAFETY: a static of the framework.
  CFRunLoop::main()?.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });

  unsafe extern "C-unwind" fn locked(
    _: *mut CFNotificationCenter,
    _: *mut c_void,
    _: *const CFNotificationName,
    _: *const c_void,
    _: *const CFDictionary,
  ) {
    if let Some(on) = ON.with(|cell| cell.get().copied()) {
      on(Change::Locked);
    }
  }

  // loginwindow's, in no SDK header. AppKit suspends the distributed center's delivery while the
  // app isn't active, and an accessory app nearly never is: this one asks for it at once.
  let name = CFString::from_static_str("com.apple.screenIsLocked");
  // SAFETY: the observer is a static's address; the callback runs on the main thread's run loop,
  // where `ON` is the app's.
  unsafe {
    CFNotificationCenter::distributed_center()?.add_observer(
      &raw const LOCK as *const c_void,
      Some(locked),
      Some(&name),
      std::ptr::null(),
      CFNotificationSuspensionBehavior::DeliverImmediately,
    );
  }

  let (center, workspace) =
    (NSNotificationCenter::defaultCenter(), NSWorkspace::sharedWorkspace().notificationCenter());
  // SAFETY: statics of the frameworks.
  let observers = unsafe {
    vec![
      observe(center.clone(), NSProcessInfoPowerStateDidChangeNotification, Change::LowPowerMode),
      observe(center, NSSystemClockDidChangeNotification, Change::Clock),
      observe(workspace, NSWorkspaceDidWakeNotification, Change::Woke),
    ]
  };

  Some(Watch { source, observers })
}

/// Hears `name` from `center` on the main queue: the power state's notification comes from any
/// thread.
fn observe(
  center: Retained<NSNotificationCenter>,
  name: &NSNotificationName,
  change: Change,
) -> Observer {
  let block = RcBlock::new(move |_: NonNull<NSNotification>| {
    if let Some(on) = ON.with(|cell| cell.get().copied()) {
      on(change);
    }
  });

  // SAFETY: the block runs on the main queue, where `ON` is the app's.
  let token = unsafe {
    center.addObserverForName_object_queue_usingBlock(
      Some(name),
      None,
      Some(&NSOperationQueue::mainQueue()),
      &block,
    )
  };

  (center, token)
}

impl Drop for Watch {
  fn drop(&mut self) {
    self.source.invalidate();
    for (center, token) in &self.observers {
      // SAFETY: a token of this center.
      unsafe { center.removeObserver(token.as_ref()) };
    }

    if let Some(center) = CFNotificationCenter::distributed_center() {
      // SAFETY: the observer added in `watch`.
      unsafe { center.remove_every_observer(&raw const LOCK as *const c_void) };
    }
  }
}
