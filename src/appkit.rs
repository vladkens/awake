// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! AppKit for what nimbo has no API for: an SF Symbol as the status item's icon (nimbo's
//! [`TrayIcon`] takes pixels or path data).

use nimbo::tray::TrayIcon;
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage};
use objc2_foundation::{NSRect, NSString};

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
