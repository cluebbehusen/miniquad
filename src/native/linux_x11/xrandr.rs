//! The refresh rate of the CRTC showing most of the window, read through
//! XRandR. The library is loaded at runtime; without it, or without the
//! server extension, the refresh rate stays unknown.

#![allow(non_upper_case_globals, non_snake_case)]

use super::libx11::{Display, LibX11, Time, Window, XID};
use core::ffi::{c_char, c_int, c_uint, c_ulong, c_ushort};
use std::time::{Duration, Instant};

pub type RRCrtc = XID;
pub type RRMode = XID;
pub type RROutput = XID;
pub type Rotation = c_ushort;
pub type XRRModeFlags = c_ulong;

pub const RR_Interlace: XRRModeFlags = 0x10;
pub const RR_DoubleScan: XRRModeFlags = 0x20;

pub const RRScreenChangeNotify: c_int = 0;
pub const RRNotify: c_int = 1;

pub const RRScreenChangeNotifyMask: c_int = 1 << 0;
pub const RRCrtcChangeNotifyMask: c_int = 1 << 1;
pub const RROutputChangeNotifyMask: c_int = 1 << 2;

/// How often everything is read again, for servers that change modes
/// without sending RandR events.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[repr(C)]
pub struct XRRModeInfo {
    pub id: RRMode,
    pub width: c_uint,
    pub height: c_uint,
    pub dotClock: c_ulong,
    pub hSyncStart: c_uint,
    pub hSyncEnd: c_uint,
    pub hTotal: c_uint,
    pub hSkew: c_uint,
    pub vSyncStart: c_uint,
    pub vSyncEnd: c_uint,
    pub vTotal: c_uint,
    pub name: *mut c_char,
    pub nameLength: c_uint,
    pub modeFlags: XRRModeFlags,
}

#[repr(C)]
pub struct XRRScreenResources {
    pub timestamp: Time,
    pub configTimestamp: Time,
    pub ncrtc: c_int,
    pub crtcs: *mut RRCrtc,
    pub noutput: c_int,
    pub outputs: *mut RROutput,
    pub nmode: c_int,
    pub modes: *mut XRRModeInfo,
}

#[repr(C)]
pub struct XRRCrtcInfo {
    pub timestamp: Time,
    pub x: c_int,
    pub y: c_int,
    pub width: c_uint,
    pub height: c_uint,
    pub mode: RRMode,
    pub rotation: Rotation,
    pub noutput: c_int,
    pub outputs: *mut RROutput,
    pub rotations: Rotation,
    pub npossible: c_int,
    pub possible: *mut RROutput,
}

crate::declare_module!(
    LibXrandr,
    "libXrandr.so.2",
    "libXrandr.so",
    ...
    ...
    pub fn XRRQueryExtension(*mut Display, *mut c_int, *mut c_int) -> c_int,
    pub fn XRRQueryVersion(*mut Display, *mut c_int, *mut c_int) -> c_int,
    pub fn XRRSelectInput(*mut Display, Window, c_int),
    pub fn XRRGetScreenResourcesCurrent(*mut Display, Window) -> *mut XRRScreenResources,
    pub fn XRRFreeScreenResources(*mut XRRScreenResources),
    pub fn XRRGetCrtcInfo(*mut Display, *mut XRRScreenResources, RRCrtc) -> *mut XRRCrtcInfo,
    pub fn XRRFreeCrtcInfo(*mut XRRCrtcInfo),
    ...
    ...
);

/// A rectangle in root window coordinates.
struct Rect {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl Rect {
    fn overlap(&self, other: &Rect) -> i64 {
        let width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let height = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        width.max(0) * height.max(0)
    }
}

/// An active CRTC: where it scans out from the root window, at what rate.
struct Crtc {
    rect: Rect,
    rate: Option<f64>,
}

pub struct Xrandr {
    lib: LibXrandr,
    event_base: c_int,
    crtcs: Vec<Crtc>,
    crtcs_changed: bool,
    window_moved: bool,
    last_poll: Option<Instant>,
    reported: Option<f64>,
}

impl Xrandr {
    /// Loads XRandR and selects its screen, CRTC and output change events
    /// on `root`. Returns `None` without the library or a 1.3 server
    /// extension, which `XRRGetScreenResourcesCurrent` needs to avoid
    /// probing the outputs.
    pub unsafe fn new(display: *mut Display, root: Window) -> Option<Xrandr> {
        let lib = LibXrandr::try_load().ok()?;
        // The first query hands Xlib hooks into libXrandr, even when the
        // server lacks the extension, and Xlib calls them as late as
        // XCloseDisplay, so the library must never be unloaded.
        std::mem::forget(lib.clone());

        let mut event_base = 0;
        let mut error_base = 0;
        if (lib.XRRQueryExtension)(display, &mut event_base, &mut error_base) == 0 {
            return None;
        }
        let mut major = 0;
        let mut minor = 0;
        if (lib.XRRQueryVersion)(display, &mut major, &mut minor) == 0 || (major, minor) < (1, 3) {
            return None;
        }

        (lib.XRRSelectInput)(
            display,
            root,
            RRScreenChangeNotifyMask | RRCrtcChangeNotifyMask | RROutputChangeNotifyMask,
        );

        Some(Xrandr {
            lib,
            event_base,
            crtcs: Vec::new(),
            crtcs_changed: true,
            window_moved: true,
            last_poll: None,
            reported: None,
        })
    }

    /// Notes a RandR change event. Other event types are ignored.
    pub fn handle_event(&mut self, event_type: c_int) {
        if event_type == self.event_base + RRScreenChangeNotify
            || event_type == self.event_base + RRNotify
        {
            self.crtcs_changed = true;
        }
    }

    pub fn window_moved(&mut self) {
        self.window_moved = true;
    }

    /// Re-reads whatever changed since the last call, and everything once a
    /// second, then reports the refresh rate of the CRTC showing most of
    /// the window. While no CRTC shows any of it, the last rate stays.
    pub unsafe fn update(
        &mut self,
        libx11: &mut LibX11,
        display: *mut Display,
        root: Window,
        window: Window,
    ) {
        if self
            .last_poll
            .is_none_or(|last| last.elapsed() >= POLL_INTERVAL)
        {
            self.last_poll = Some(Instant::now());
            self.crtcs_changed = true;
        }
        if self.crtcs_changed {
            self.crtcs_changed = false;
            // A CRTC destroyed between the resources reply and its info
            // request (a monitor unplugged mid-read) raises BadRRCrtc, and
            // Xlib's default handler would exit the process.
            libx11.grab_error_handler();
            self.crtcs = self.read_crtcs(display, root);
            libx11.release_error_handler(display);
            self.window_moved = true;
        }
        if !self.window_moved {
            return;
        }
        self.window_moved = false;

        let mut x = 0;
        let mut y = 0;
        let mut child: Window = 0;
        if (libx11.XTranslateCoordinates)(display, window, root, 0, 0, &mut x, &mut y, &mut child)
            == 0
        {
            return;
        }
        let (width, height) = {
            let d = crate::native_display().lock().unwrap();
            (d.screen_width, d.screen_height)
        };
        let window = Rect {
            x: x as i64,
            y: y as i64,
            width: width as i64,
            height: height as i64,
        };
        if let Some(crtc) = most_overlapping(&self.crtcs, &window) {
            if crtc.rate != self.reported {
                self.reported = crtc.rate;
                crate::native_display().lock().unwrap().refresh_rate = crtc.rate;
            }
        }
    }

    unsafe fn read_crtcs(&self, display: *mut Display, root: Window) -> Vec<Crtc> {
        let resources = (self.lib.XRRGetScreenResourcesCurrent)(display, root);
        if resources.is_null() {
            return Vec::new();
        }
        let modes = raw_slice((*resources).modes, (*resources).nmode);
        let mut crtcs = Vec::new();
        for &id in raw_slice((*resources).crtcs, (*resources).ncrtc) {
            let info = (self.lib.XRRGetCrtcInfo)(display, resources, id);
            if info.is_null() {
                continue;
            }
            let crtc_info = &*info;
            if crtc_info.mode != 0 {
                crtcs.push(Crtc {
                    rect: Rect {
                        x: crtc_info.x as i64,
                        y: crtc_info.y as i64,
                        width: crtc_info.width as i64,
                        height: crtc_info.height as i64,
                    },
                    rate: modes
                        .iter()
                        .find(|mode| mode.id == crtc_info.mode)
                        .and_then(mode_rate),
                });
            }
            (self.lib.XRRFreeCrtcInfo)(info);
        }
        (self.lib.XRRFreeScreenResources)(resources);
        crtcs
    }
}

unsafe fn raw_slice<'a, T>(ptr: *const T, len: c_int) -> &'a [T] {
    if ptr.is_null() || len <= 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len as usize)
    }
}

/// The vertical refresh rate of a mode, as xrandr computes it: interlaced
/// modes count fields rather than frames, and doublescan sends each line
/// twice.
fn mode_rate(mode: &XRRModeInfo) -> Option<f64> {
    let mut v_total = mode.vTotal as f64;
    if mode.modeFlags & RR_DoubleScan != 0 {
        v_total *= 2.0;
    }
    if mode.modeFlags & RR_Interlace != 0 {
        v_total /= 2.0;
    }
    let pixels = mode.hTotal as f64 * v_total;
    (mode.dotClock > 0 && pixels > 0.0).then(|| mode.dotClock as f64 / pixels)
}

/// The first of the CRTCs that overlap `window` the most, or `None` when
/// none overlaps it.
fn most_overlapping<'a>(crtcs: &'a [Crtc], window: &Rect) -> Option<&'a Crtc> {
    let mut best = None;
    let mut best_overlap = 0;
    for crtc in crtcs {
        let overlap = crtc.rect.overlap(window);
        if overlap > best_overlap {
            best = Some(crtc);
            best_overlap = overlap;
        }
    }
    best
}
