// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! AppKit for what nimbo has no API for: an SF Symbol as the status item's icon (nimbo's
//! [`TrayIcon`] takes pixels or path data), the status line, toggle and checkmarks of the menu
//! while it is open (nimbo builds a menu set meanwhile only once it closes, and tells nothing of
//! its opening or closing), and the time of day as the user's clock shows it.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::time::{SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use nimbo::tray::TrayIcon;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
  NSApplication, NSBitmapImageRep, NSControlStateValueOff, NSControlStateValueOn,
  NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSMenu, NSMenuDidBeginTrackingNotification,
  NSMenuDidEndTrackingNotification,
};
use objc2_foundation::{
  NSDate, NSDateFormatter, NSDateFormatterStyle, NSNotification, NSNotificationCenter, NSRect,
  NSString,
};

thread_local! {
  /// The status item's menu while it is open.
  static OPEN: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
}

/// The first of the SF Symbols `symbols` the system has, drawn at twice its size in points: a
/// template of its alpha, as the menu bar tints it. Blank when it has none.
pub fn symbol(symbols: &[&str]) -> TrayIcon {
  let blank = TrayIcon::Rgba { w: 0, h: 0, scale: 2.0, px: Vec::new() };
  let image = symbols.iter().find_map(|symbol| {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None)
  });
  let Some(image) = image else { return blank };

  let size = image.size();
  let (w, h) = ((size.width * 2.0).ceil() as isize, (size.height * 2.0).ceil() as isize);
  // SAFETY: no planes: the rep allocates its own buffer, 4 bytes a pixel, rows top-down.
  let rep = unsafe {
    NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
      NSBitmapImageRep::alloc(),
      std::ptr::null_mut(),
      w,
      h,
      8,
      4,
      true,
      false,
      NSDeviceRGBColorSpace,
      w * 4,
      32,
    )
  };
  let Some(rep) = rep else { return blank };

  rep.setSize(size);
  NSGraphicsContext::saveGraphicsState_class();
  NSGraphicsContext::setCurrentContext(
    NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep).as_deref(),
  );
  image.drawInRect(NSRect::new(Default::default(), size));
  NSGraphicsContext::restoreGraphicsState_class();

  let data = rep.bitmapData();
  if data.is_null() {
    return blank;
  }

  // SAFETY: the rep's buffer, `w * 4` bytes by `h` rows, alive while `rep` is.
  let px = unsafe { std::slice::from_raw_parts(data, (w * h * 4) as usize) }.to_vec();
  TrayIcon::Rgba { w: w as u32, h: h as u32, scale: 2.0, px }
}

/// `on` hears the status item's menu open (true) and close (false): a menu tracked that is not the
/// app's menu bar. For good: the notification center keeps the blocks.
pub fn watch_menu(on: fn(bool)) {
  let center = NSNotificationCenter::defaultCenter();
  // SAFETY: statics of the framework.
  let names = unsafe {
    [(NSMenuDidBeginTrackingNotification, true), (NSMenuDidEndTrackingNotification, false)]
  };

  for (name, open) in names {
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
      let Some(mtm) = MainThreadMarker::new() else { return };
      // SAFETY: AppKit posts it with the menu.
      let notification = unsafe { notification.as_ref() };
      let Some(menu) = notification.object().and_then(|o| o.downcast::<NSMenu>().ok()) else {
        return;
      };

      // nimbo doesn't tell its tray's menu; the app has no other but its menu bar.
      if NSApplication::sharedApplication(mtm).mainMenu().is_some_and(|bar| bar == menu) {
        return;
      }

      OPEN.set(open.then_some(menu));
      on(open);
    });

    // SAFETY: the block runs where it is posted, the main thread.
    let _ =
      unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
  }
}

/// The open menu as it changes: the titles of its first item (the status line) and of the toggle
/// after the divider under it, and the checkmarks in the submenu after the toggle.
pub fn show_open(status: &str, toggle: &str, checks: impl Iterator<Item = bool>) {
  OPEN.with_borrow(|menu| {
    let Some(menu) = menu else { return };
    for (i, title) in [(0, status), (2, toggle)] {
      let Some(item) = menu.itemAtIndex(i) else { continue };
      let title = NSString::from_str(title);
      if item.title() != title {
        item.setTitle(&title);
      }
    }

    let Some(submenu) = menu.itemAtIndex(3).and_then(|item| item.submenu()) else { return };
    for (i, on) in (0..).zip(checks) {
      if let Some(item) = submenu.itemAtIndex(i) {
        item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
      }
    }
  });
}

/// `t`'s time of day as the user's clock shows it: `18:42`, `6:42 PM`.
pub fn clock(t: SystemTime) -> String {
  let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
  let format = NSDateFormatter::new();
  format.setDateStyle(NSDateFormatterStyle::NoStyle);
  format.setTimeStyle(NSDateFormatterStyle::ShortStyle);
  format.stringFromDate(&NSDate::dateWithTimeIntervalSince1970(secs)).to_string()
}
