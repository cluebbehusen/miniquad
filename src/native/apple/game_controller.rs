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
    std::cell::{Cell, RefCell},
};

#[cfg(test)]
mod tests;

thread_local! {
    /// The controllers the last read listed. Retaining them keeps a
    /// departed controller's address from passing to a new one while it
    /// is still listed here.
    static LISTED: RefCell<Vec<(RcObjcId, ControllerId)>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
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
                None => (RcObjcId::from_unowned(object), next_id()),
            };
            read_controller(controller, entry.1, &mut controllers);
            current.push(entry);
        }
        // Dropping the controllers no longer listed releases them.
        *previous = current;
    });
    controllers.gamepads.sort_by_key(|pad| pad.id);
    controllers.micro_gamepads.sort_by_key(|pad| pad.id);
    controllers
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
    let pressed: BOOL = msg_send![button, isPressed];
    Some(pressed != NO)
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
    let buttons = [
        (MicroButton::Select, sel!(buttonA)),
        (MicroButton::PlayPause, sel!(buttonX)),
        (MicroButton::Menu, sel!(buttonMenu)),
    ];
    for (button, getter) in buttons {
        if let Some(down) = pressed(profile, getter) {
            micro.set(button, down);
        }
    }
    micro
}
