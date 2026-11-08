//! Game controllers on macOS, iOS and tvOS, read through Apple's
//! GameController framework.
//!
//! Call [`controllers`] once a frame from `update` or `draw`. It returns
//! each connected controller's state at that moment, so a press that
//! starts and ends between two calls is not seen, apart from a micro
//! gamepad's [`MicroButton`]s, and nothing is read while the app draws no
//! frames. A controller keeps its [`ControllerId`] while it
//! stays connected: compare ids with the previous frame's to see
//! controllers connect and disconnect.
//!
//! Full gamepads and micro gamepads, such as the Siri Remote, arrive in
//! separate lists.
//!
//! On tvOS, controller presses stay away from UIKit until
//! [`set_system_receives_presses`] lets them through. While the system
//! keyboard is up its presses are the keyboard's: controllers read as
//! untouched then, and until a press that outlives the keyboard is let go.

use crate::native::apple::game_controller;

#[cfg(test)]
mod tests;

/// Names one connected controller. A controller that disconnects and
/// connects again gets a new id; ids are never reused.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ControllerId(pub(crate) u64);

/// Whose labels a gamepad's buttons carry, for choosing button icons.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Family {
    Xbox,
    PlayStation,
    Switch,
    Other,
}

impl Family {
    /// The family of a controller with this GameController product
    /// category, such as "DualSense" or "Switch Pro Controller".
    pub(crate) fn from_product_category(category: &str) -> Family {
        if category.contains("Xbox") {
            Family::Xbox
        } else if category.contains("DualShock") || category.contains("DualSense") {
            Family::PlayStation
        } else if category.contains("Switch") || category.contains("Joy-Con") {
            Family::Switch
        } else {
            Family::Other
        }
    }
}

/// A button on a full gamepad. The face buttons are named by position,
/// as GameController reports them: South is A on Xbox and Cross on
/// PlayStation.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    /// Pulled past the system's threshold; [`Gamepad::left_trigger`]
    /// has the analog value.
    LeftTrigger,
    /// Pulled past the system's threshold; [`Gamepad::right_trigger`]
    /// has the analog value.
    RightTrigger,
    /// Clicking the left stick.
    LeftStick,
    /// Clicking the right stick.
    RightStick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    /// Menu on Xbox, Options on PlayStation, + on Switch. Needs iOS and
    /// tvOS 13 or macOS 10.15; older systems report it missing.
    Menu,
    /// View on Xbox, Share or Create on PlayStation, − on Switch.
    Options,
    /// The Xbox, PlayStation or Home button. The system may act on it
    /// instead of reporting it.
    Home,
}

impl Button {
    pub const ALL: [Button; 17] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::LeftShoulder,
        Button::RightShoulder,
        Button::LeftTrigger,
        Button::RightTrigger,
        Button::LeftStick,
        Button::RightStick,
        Button::DpadUp,
        Button::DpadDown,
        Button::DpadLeft,
        Button::DpadRight,
        Button::Menu,
        Button::Options,
        Button::Home,
    ];

    fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// A button on a micro gamepad, named as on the Siri Remote. tvOS can make a
/// press too short to poll, so one between two reads still reads as pressed
/// once.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum MicroButton {
    /// Pressing anywhere on the touch surface, whichever part of it.
    Select,
    PlayPause,
    /// Back on the Siri Remote. Needs iOS and tvOS 13 or macOS 10.15; older
    /// systems report it missing.
    Menu,
    /// Pressing the center of a remote that can tell it from the ring around
    /// it, such as the inner ring of the 2021 Siri Remote. Only tvOS reports
    /// this; a remote that can't tell reports it missing.
    Center,
    /// Pressing the top of the ring around [`MicroButton::Center`]. Reported
    /// the same way, as are the other three directions.
    Up,
    Down,
    Left,
    Right,
}

impl MicroButton {
    pub const ALL: [MicroButton; 8] = [
        MicroButton::Select,
        MicroButton::PlayPause,
        MicroButton::Menu,
        MicroButton::Center,
        MicroButton::Up,
        MicroButton::Down,
        MicroButton::Left,
        MicroButton::Right,
    ];

    pub(crate) const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A stick or touch-surface position: x to the right and y up, each
/// from -1 to 1.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Direction {
    pub x: f32,
    pub y: f32,
}

/// A full gamepad's state: two sticks, two analog triggers, a d-pad
/// and the [`Button`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct Gamepad {
    pub id: ControllerId,
    pub family: Family,
    /// The name the system gives the controller, such as "Xbox Wireless
    /// Controller".
    pub name: Option<String>,
    pub left_stick: Direction,
    pub right_stick: Direction,
    /// From 0 when released to 1 when fully pulled.
    pub left_trigger: f32,
    /// From 0 when released to 1 when fully pulled.
    pub right_trigger: f32,
    pub(crate) pressed: u32,
    pub(crate) present: u32,
}

impl Gamepad {
    pub fn is_pressed(&self, button: Button) -> bool {
        self.pressed & button.bit() != 0
    }

    /// Whether this gamepad has the button. Stick clicks, Options and
    /// Home can be missing, and Menu before iOS and tvOS 13 or macOS
    /// 10.15; every gamepad has the rest.
    pub fn has(&self, button: Button) -> bool {
        self.present & button.bit() != 0
    }

    pub(crate) fn set(&mut self, button: Button, pressed: bool) {
        self.present |= button.bit();
        if pressed {
            self.pressed |= button.bit();
        }
    }
}

/// A micro gamepad's state, such as the Siri Remote's: one touch
/// surface or d-pad and the [`MicroButton`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct MicroGamepad {
    pub id: ControllerId,
    /// The name the system gives the controller, such as "Siri Remote".
    pub name: Option<String>,
    /// Where the touch is on the surface, from -1 to 1 across it; zero
    /// while nothing touches it. Measure a swipe from where the touch
    /// began, since a touch can begin anywhere.
    pub touch: Direction,
    pub(crate) pressed: u8,
    pub(crate) present: u8,
}

impl MicroGamepad {
    pub fn is_pressed(&self, button: MicroButton) -> bool {
        self.pressed & button.bit() != 0
    }

    /// Whether this controller has the button.
    pub fn has(&self, button: MicroButton) -> bool {
        self.present & button.bit() != 0
    }

    pub(crate) fn set(&mut self, button: MicroButton, pressed: bool) {
        self.present |= button.bit();
        if pressed {
            self.pressed |= button.bit();
        }
    }
}

/// Every connected controller, each list ordered from the earliest
/// connection.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Controllers {
    pub gamepads: Vec<Gamepad>,
    pub micro_gamepads: Vec<MicroGamepad>,
}

/// Reads every connected controller. Call it on the main thread, where
/// `update` and `draw` run.
pub fn controllers() -> Controllers {
    game_controller::read()
}

/// Whether UIKit also receives controller presses. It starts off, so
/// presses reach only [`controllers`]. On, the system acts on them: Back
/// on the Siri Remote returns to the Home screen, which tvOS expects from
/// an app's top screen. The system keyboard gets them while it is up
/// whatever this says. Takes effect on the next frame. Does nothing off
/// tvOS.
#[cfg_attr(not(target_os = "tvos"), allow(unused_variables))]
pub fn set_system_receives_presses(receives: bool) {
    #[cfg(target_os = "tvos")]
    crate::native_display()
        .lock()
        .unwrap()
        .native_requests
        .send(crate::native::Request::SetSystemReceivesPresses(receives))
        .unwrap();
}
