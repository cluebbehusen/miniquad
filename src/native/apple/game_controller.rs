//! Reads `GCController`s for [`crate::gamepad`].

use {
    crate::{
        gamepad::{
            Button, ControllerId, Controllers, Direction, Family, Gamepad, MicroButton,
            MicroGamepad,
        },
        native::apple::{apple_util::nsstring_to_string, frameworks::*},
    },
    objc::Message,
    std::{
        cell::{Cell, RefCell},
        ffi::c_void,
        sync::atomic::{AtomicU8, Ordering},
    },
};

#[cfg(test)]
mod tests;

thread_local! {
    /// The controllers the last read listed. Retaining them keeps a
    /// departed controller's address from passing to a new one while it
    /// is still listed here.
    static LISTED: RefCell<Vec<(RcObjcId, ControllerId)>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
    /// The system keyboard is up, so controller presses are its own.
    static KEYBOARD_UP: Cell<bool> = const { Cell::new(false) };
    /// Presses are hidden until nothing is pressed.
    static HIDING_PRESSES: Cell<bool> = const { Cell::new(false) };
}

/// How far past this a stick or trigger must be from rest to count as still
/// held. A worn stick never quite returns to zero, so it can't be nothing.
const ANALOG_REST: f32 = 0.2;

/// Says whether the system keyboard is up. Hiding starts as it opens, since a
/// press can outlive it without any read seeing the keyboard up.
#[cfg(any(target_os = "tvos", test))]
pub(crate) fn set_keyboard_up(up: bool) {
    KEYBOARD_UP.with(|cell| cell.set(up));
    if up {
        HIDING_PRESSES.with(|cell| cell.set(true));
    }
}

/// Whether the system keyboard is up.
#[cfg(target_os = "tvos")]
pub(crate) fn keyboard_up() -> bool {
    KEYBOARD_UP.with(Cell::get)
}

/// The [`MicroButton`]s that went down since the last read took them.
static LATCHED: AtomicU8 = AtomicU8::new(0);

/// A global block as the compiler lays one out: it captures nothing, so
/// the runtime never copies or frees it.
#[repr(C)]
struct ButtonHandler {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*const ButtonHandler, ObjcId, f32, BOOL),
    descriptor: &'static BlockDescriptor,
}

// Its fields never change.
unsafe impl Sync for ButtonHandler {}

#[repr(C)]
struct BlockDescriptor {
    reserved: usize,
    size: usize,
}

const BLOCK_IS_GLOBAL: i32 = 1 << 28;

macro_rules! button_handler {
    ($button:expr) => {
        ButtonHandler {
            isa: &raw const _NSConcreteGlobalBlock as *const c_void,
            flags: BLOCK_IS_GLOBAL,
            reserved: 0,
            invoke: latch::<{ $button.bit() }>,
            descriptor: &BlockDescriptor {
                reserved: 0,
                size: std::mem::size_of::<ButtonHandler>(),
            },
        }
    };
}

/// The `pressedChangedHandler` of each micro gamepad button, in
/// [`MicroButton`] order. tvOS runs gesture recognizers over these buttons
/// before the app hears of them, which can leave too short a window to poll,
/// as `GCMicroGamepad.h` warns, and a slow frame widens the gap between polls.
static HANDLERS: [ButtonHandler; MicroButton::ALL.len()] = [
    button_handler!(MicroButton::Select),
    button_handler!(MicroButton::PlayPause),
    button_handler!(MicroButton::Menu),
    button_handler!(MicroButton::Center),
    button_handler!(MicroButton::Up),
    button_handler!(MicroButton::Down),
    button_handler!(MicroButton::Left),
    button_handler!(MicroButton::Right),
];

unsafe extern "C" fn latch<const BIT: u8>(
    _block: *const ButtonHandler,
    _button: ObjcId,
    _value: f32,
    pressed: BOOL,
) {
    if pressed != NO {
        LATCHED.fetch_or(BIT, Ordering::Relaxed);
    }
}

pub(crate) fn read() -> Controllers {
    objc::rc::autoreleasepool(|| unsafe { read_in_pool() })
}

unsafe fn read_in_pool() -> Controllers {
    let main_thread: BOOL = msg_send![class!(NSThread), isMainThread];
    debug_assert!(main_thread != NO, "read controllers on the main thread");

    let connected: ObjcId = msg_send![class!(GCController), controllers];
    let count: usize = msg_send![connected, count];
    let mut controllers = Controllers::default();
    LISTED.with(|listed| {
        let mut previous = listed.borrow_mut();
        let mut current = Vec::with_capacity(count);
        for index in 0..count {
            let controller: ObjcId = msg_send![connected, objectAtIndex: index];
            let Some(object) = NonNull::new(controller) else {
                continue;
            };
            let entry = match previous.iter().position(|(c, _)| c.as_id() == controller) {
                Some(at) => previous.swap_remove(at),
                None => {
                    prepare_micro_gamepad(controller);
                    (RcObjcId::from_unowned(object), next_id())
                }
            };
            read_controller(controller, entry.1, &mut controllers);
            current.push(entry);
        }
        // Dropping the controllers no longer listed releases them.
        *previous = current;
    });
    controllers.gamepads.sort_by_key(|pad| pad.id);
    controllers.micro_gamepads.sort_by_key(|pad| pad.id);
    take_latched_presses(&mut controllers);
    hide_keyboard_presses(&mut controllers);
    controllers
}

/// Leaves out what the system keyboard is using, so a click on one of its
/// keys never also presses something behind it. A press that outlives the
/// keyboard, as the Back that closes it does, stays hidden until let go.
fn hide_keyboard_presses(controllers: &mut Controllers) {
    let stick_held = |at: Direction| at.x.hypot(at.y) > ANALOG_REST;
    let touched = controllers.gamepads.iter().any(|pad| {
        pad.pressed != 0
            || stick_held(pad.left_stick)
            || stick_held(pad.right_stick)
            || pad.left_trigger.max(pad.right_trigger) > ANALOG_REST
    }) || controllers
        .micro_gamepads
        .iter()
        .any(|micro| micro.pressed != 0 || micro.touch != Direction::default());
    let hide = KEYBOARD_UP.with(Cell::get) || (HIDING_PRESSES.with(Cell::get) && touched);
    HIDING_PRESSES.with(|cell| cell.set(hide));
    if hide {
        for pad in &mut controllers.gamepads {
            pad.pressed = 0;
            pad.left_stick = Direction::default();
            pad.right_stick = Direction::default();
            pad.left_trigger = 0.;
            pad.right_trigger = 0.;
        }
        for micro in &mut controllers.micro_gamepads {
            micro.pressed = 0;
            micro.touch = Direction::default();
        }
    }
}

/// Sets up a newly listed controller if it is a micro gamepad. Its touch
/// reports where the finger is, not a window that slides with the finger,
/// so a swipe's travel can be measured and a resting finger reads as still.
unsafe fn prepare_micro_gamepad(controller: ObjcId) {
    if !element(controller, sel!(extendedGamepad)).is_null() {
        return;
    }
    let micro = element(controller, sel!(microGamepad));
    if micro.is_null() {
        return;
    }
    let () = msg_send![micro, setReportsAbsoluteDpadValues: YES];
    for (button, input) in micro_inputs(micro) {
        if !input.is_null() {
            let handler: *const ButtonHandler = &HANDLERS[button as usize];
            let () = msg_send![input, setPressedChangedHandler: handler];
        }
    }
}

/// Reports the presses the handlers saw, however briefly they lasted. A
/// handler doesn't say which remote it was, so the first one with the button
/// takes it, unless a remote is reporting that button itself.
fn take_latched_presses(controllers: &mut Controllers) {
    let latched = LATCHED.swap(0, Ordering::Relaxed);
    for button in MicroButton::ALL {
        let reported = |micro: &MicroGamepad| micro.is_pressed(button);
        if latched & button.bit() != 0 && !controllers.micro_gamepads.iter().any(reported) {
            if let Some(micro) = controllers
                .micro_gamepads
                .iter_mut()
                .find(|micro| micro.has(button))
            {
                micro.set(button, true);
            }
        }
    }
}

/// Each [`MicroButton`]'s input on a micro gamepad profile, nil where this OS
/// or remote lacks it.
unsafe fn micro_inputs(profile: ObjcId) -> [(MicroButton, ObjcId); MicroButton::ALL.len()] {
    let (center, ring) = directional_inputs(profile);
    let on_ring = |getter| {
        if ring.is_null() {
            nil
        } else {
            element(ring, getter)
        }
    };
    [
        (MicroButton::Select, element(profile, sel!(buttonA))),
        (MicroButton::PlayPause, element(profile, sel!(buttonX))),
        (MicroButton::Menu, element(profile, sel!(buttonMenu))),
        (MicroButton::Center, center),
        (MicroButton::Up, on_ring(sel!(up))),
        (MicroButton::Down, on_ring(sel!(down))),
        (MicroButton::Left, on_ring(sel!(left))),
        (MicroButton::Right, on_ring(sel!(right))),
    ]
}

/// A remote's center button and the ring of direction buttons around it. A
/// remote without them, as every remote is off tvOS, has only its surface.
#[cfg(target_os = "tvos")]
unsafe fn directional_inputs(profile: ObjcId) -> (ObjcId, ObjcId) {
    let named = |getter, name: ObjcId| {
        let inputs = element(profile, getter);
        if inputs.is_null() {
            nil
        } else {
            let input: ObjcId = msg_send![inputs, objectForKey: name];
            input
        }
    };
    (
        named(sel!(buttons), GCInputDirectionalCenterButton),
        named(sel!(dpads), GCInputDirectionalCardinalDpad),
    )
}

#[cfg(not(target_os = "tvos"))]
unsafe fn directional_inputs(_profile: ObjcId) -> (ObjcId, ObjcId) {
    (nil, nil)
}

/// Adds a `GCController`'s state to the list its profile belongs in.
/// A controller with neither profile is left out.
unsafe fn read_controller(controller: ObjcId, id: ControllerId, controllers: &mut Controllers) {
    let name = optional_string(controller, sel!(vendorName));
    let extended = element(controller, sel!(extendedGamepad));
    if !extended.is_null() {
        controllers
            .gamepads
            .push(read_gamepad(id, name, family(controller), extended));
        return;
    }
    let micro = element(controller, sel!(microGamepad));
    if !micro.is_null() {
        controllers
            .micro_gamepads
            .push(read_micro_gamepad(id, name, micro));
    }
}

fn next_id() -> ControllerId {
    NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        ControllerId(id)
    })
}

/// What `getter` returns, or nil where this OS or controller lacks it.
unsafe fn element(object: ObjcId, getter: Sel) -> ObjcId {
    let responds: BOOL = msg_send![object, respondsToSelector: getter];
    if responds == NO {
        return nil;
    }
    (*object).send_message(getter, ()).unwrap_or(nil)
}

unsafe fn optional_string(object: ObjcId, getter: Sel) -> Option<String> {
    let string = element(object, getter);
    if string.is_null() {
        None
    } else {
        Some(nsstring_to_string(string))
    }
}

unsafe fn family(controller: ObjcId) -> Family {
    match optional_string(controller, sel!(productCategory)) {
        Some(category) => Family::from_product_category(&category),
        None => Family::Other,
    }
}

/// Whether a `GCControllerButtonInput` is pressed, or `None` if the
/// controller lacks it.
unsafe fn pressed(input: ObjcId, getter: Sel) -> Option<bool> {
    let button = element(input, getter);
    if button.is_null() {
        return None;
    }
    Some(is_pressed(button))
}

unsafe fn is_pressed(button: ObjcId) -> bool {
    let pressed: BOOL = msg_send![button, isPressed];
    pressed != NO
}

unsafe fn analog(input: ObjcId, getter: Sel) -> f32 {
    let button = element(input, getter);
    if button.is_null() {
        return 0.;
    }
    msg_send![button, value]
}

/// A `GCControllerDirectionPad`'s axes.
unsafe fn direction(input: ObjcId, getter: Sel) -> Direction {
    let pad = element(input, getter);
    if pad.is_null() {
        return Direction::default();
    }
    let x_axis: ObjcId = msg_send![pad, xAxis];
    let y_axis: ObjcId = msg_send![pad, yAxis];
    Direction {
        x: msg_send![x_axis, value],
        y: msg_send![y_axis, value],
    }
}

unsafe fn read_gamepad(
    id: ControllerId,
    name: Option<String>,
    family: Family,
    profile: ObjcId,
) -> Gamepad {
    let mut gamepad = Gamepad {
        id,
        family,
        name,
        left_stick: direction(profile, sel!(leftThumbstick)),
        right_stick: direction(profile, sel!(rightThumbstick)),
        left_trigger: analog(profile, sel!(leftTrigger)),
        right_trigger: analog(profile, sel!(rightTrigger)),
        pressed: 0,
        present: 0,
    };
    let buttons = [
        (Button::South, sel!(buttonA)),
        (Button::East, sel!(buttonB)),
        (Button::West, sel!(buttonX)),
        (Button::North, sel!(buttonY)),
        (Button::LeftShoulder, sel!(leftShoulder)),
        (Button::RightShoulder, sel!(rightShoulder)),
        (Button::LeftTrigger, sel!(leftTrigger)),
        (Button::RightTrigger, sel!(rightTrigger)),
        (Button::LeftStick, sel!(leftThumbstickButton)),
        (Button::RightStick, sel!(rightThumbstickButton)),
        (Button::Menu, sel!(buttonMenu)),
        (Button::Options, sel!(buttonOptions)),
        (Button::Home, sel!(buttonHome)),
    ];
    for (button, getter) in buttons {
        if let Some(down) = pressed(profile, getter) {
            gamepad.set(button, down);
        }
    }
    let dpad = element(profile, sel!(dpad));
    if !dpad.is_null() {
        let directions = [
            (Button::DpadUp, sel!(up)),
            (Button::DpadDown, sel!(down)),
            (Button::DpadLeft, sel!(left)),
            (Button::DpadRight, sel!(right)),
        ];
        for (button, getter) in directions {
            if let Some(down) = pressed(dpad, getter) {
                gamepad.set(button, down);
            }
        }
    }
    gamepad
}

unsafe fn read_micro_gamepad(
    id: ControllerId,
    name: Option<String>,
    profile: ObjcId,
) -> MicroGamepad {
    let mut micro = MicroGamepad {
        id,
        name,
        touch: direction(profile, sel!(dpad)),
        pressed: 0,
        present: 0,
    };
    for (button, input) in micro_inputs(profile) {
        if !input.is_null() {
            micro.set(button, is_pressed(input));
        }
    }
    micro
}
