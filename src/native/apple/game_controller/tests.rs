use super::*;

unsafe fn set_button(profile: ObjcId, getter: Sel, value: f32) {
    let button = element(profile, getter);
    let () = msg_send![button, setValue: value];
}

unsafe fn set_direction(profile: ObjcId, getter: Sel, x: f32, y: f32) {
    let pad = element(profile, getter);
    let () = msg_send![pad, setValueForXAxis: x yAxis: y];
}

#[test]
fn reads_a_gamepads_buttons_sticks_and_triggers() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithExtendedGamepad];
        let profile: ObjcId = msg_send![controller, extendedGamepad];
        set_button(profile, sel!(buttonA), 1.);
        set_button(profile, sel!(rightShoulder), 1.);
        set_button(profile, sel!(rightTrigger), 1.);
        set_button(profile, sel!(leftTrigger), 0.25);
        set_direction(profile, sel!(dpad), 0., 1.);
        set_direction(profile, sel!(leftThumbstick), 0.5, -1.);

        let mut controllers = Controllers::default();
        read_controller(controller, ControllerId(7), &mut controllers);

        assert_eq!(controllers.micro_gamepads, []);
        let [gamepad] = &controllers.gamepads[..] else {
            panic!("expected one gamepad, read {:?}", controllers.gamepads);
        };
        assert_eq!(gamepad.id, ControllerId(7));
        assert_eq!(gamepad.family, Family::Other);
        assert_eq!(gamepad.left_stick, Direction { x: 0.5, y: -1. });
        assert_eq!(gamepad.right_stick, Direction::default());
        assert_eq!(gamepad.left_trigger, 0.25);
        assert_eq!(gamepad.right_trigger, 1.);
        let held = [
            Button::South,
            Button::RightShoulder,
            Button::RightTrigger,
            Button::DpadUp,
        ];
        for button in Button::ALL {
            if button != Button::LeftTrigger {
                assert_eq!(
                    gamepad.is_pressed(button),
                    held.contains(&button),
                    "{button:?}"
                );
            }
        }
    });
}

#[test]
fn reads_a_micro_gamepad_apart_from_gamepads() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        let profile: ObjcId = msg_send![controller, microGamepad];
        set_button(profile, sel!(buttonX), 1.);
        set_direction(profile, sel!(dpad), -0.5, 0.);

        let mut controllers = Controllers::default();
        read_controller(controller, ControllerId(3), &mut controllers);

        assert_eq!(controllers.gamepads, []);
        let [micro] = &controllers.micro_gamepads[..] else {
            panic!(
                "expected one micro gamepad, read {:?}",
                controllers.micro_gamepads
            );
        };
        assert_eq!(micro.id, ControllerId(3));
        assert_eq!(micro.touch, Direction { x: -0.5, y: 0. });
        for button in MicroButton::ALL {
            assert!(micro.has(button), "{:?}", button);
            assert_eq!(
                micro.is_pressed(button),
                button == MicroButton::PlayPause,
                "{button:?}"
            );
        }
    });
}
