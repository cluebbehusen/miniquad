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
        for button in [
            MicroButton::Select,
            MicroButton::PlayPause,
            MicroButton::Menu,
        ] {
            assert!(micro.has(button), "{:?}", button);
            assert_eq!(
                micro.is_pressed(button),
                button == MicroButton::PlayPause,
                "{button:?}"
            );
        }
        for button in [
            MicroButton::Center,
            MicroButton::Up,
            MicroButton::Down,
            MicroButton::Left,
            MicroButton::Right,
        ] {
            assert!(!micro.has(button), "{button:?}: only tvOS reports it");
        }
    });
}

#[test]
fn a_new_micro_gamepad_reports_absolute_touches_and_hears_its_buttons() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        prepare_micro_gamepad(controller);

        let profile: ObjcId = msg_send![controller, microGamepad];
        let absolute: BOOL = msg_send![profile, reportsAbsoluteDpadValues];
        assert_ne!(absolute, NO);
        for (button, getter) in [
            (MicroButton::Select, sel!(buttonA)),
            (MicroButton::PlayPause, sel!(buttonX)),
            (MicroButton::Menu, sel!(buttonMenu)),
        ] {
            let input = element(profile, getter);
            let handler: *const c_void = msg_send![input, pressedChangedHandler];
            assert_eq!(
                handler,
                (&HANDLERS[button as usize] as *const ButtonHandler).cast()
            );
        }
    });
}

#[test]
fn a_full_gamepad_is_left_as_it_is() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithExtendedGamepad];
        prepare_micro_gamepad(controller);

        let profile: ObjcId = msg_send![controller, extendedGamepad];
        let menu = element(profile, sel!(buttonMenu));
        let handler: *const c_void = msg_send![menu, pressedChangedHandler];
        assert!(handler.is_null());
    });
}

#[test]
fn a_press_between_reads_reads_as_pressed_once_on_the_first_remote_with_that_button() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        let other: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        let read = |pressed: Option<MicroButton>| {
            let mut controllers = Controllers::default();
            read_controller(controller, ControllerId(0), &mut controllers);
            read_controller(other, ControllerId(1), &mut controllers);
            if let Some(button) = pressed {
                let handler = &HANDLERS[button as usize];
                (handler.invoke)(handler, nil, 1., YES);
                (handler.invoke)(handler, nil, 0., NO);
            }
            take_latched_presses(&mut controllers);
            controllers
                .micro_gamepads
                .iter()
                .map(|micro| {
                    MicroButton::ALL
                        .iter()
                        .copied()
                        .filter(|button| micro.is_pressed(*button))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let none: [Vec<MicroButton>; 2] = [vec![], vec![]];
        assert_eq!(read(None), none);
        assert_eq!(
            read(Some(MicroButton::Menu)),
            [vec![MicroButton::Menu], vec![]]
        );
        assert_eq!(read(None), none);
        assert_eq!(
            read(Some(MicroButton::Select)),
            [vec![MicroButton::Select], vec![]]
        );
        assert_eq!(read(Some(MicroButton::Up)), none, "no remote has the ring");
        assert_eq!(read(None), none);

        // The other remote reports its own press, so nothing doubles it.
        let profile: ObjcId = msg_send![other, microGamepad];
        set_button(profile, sel!(buttonA), 1.);
        assert_eq!(
            read(Some(MicroButton::Select)),
            [vec![], vec![MicroButton::Select]]
        );
    });
}

#[test]
fn the_keyboards_presses_stay_hidden_until_they_outlive_it_and_are_let_go() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        let profile: ObjcId = msg_send![controller, microGamepad];
        let read = || {
            let mut controllers = Controllers::default();
            read_controller(controller, ControllerId(0), &mut controllers);
            hide_keyboard_presses(&mut controllers);
            let micro = &controllers.micro_gamepads[0];
            (micro.is_pressed(MicroButton::Select), micro.touch.x)
        };
        set_button(profile, sel!(buttonA), 1.);
        set_direction(profile, sel!(dpad), 0.5, 0.);
        assert_eq!(read(), (true, 0.5), "nothing to hide yet");

        set_keyboard_up(true);
        assert_eq!(read(), (false, 0.), "the keyboard's press");

        set_keyboard_up(false);
        assert_eq!(read(), (false, 0.), "it outlives the keyboard");

        set_button(profile, sel!(buttonA), 0.);
        set_direction(profile, sel!(dpad), 0., 0.);
        assert_eq!(read(), (false, 0.));
        set_button(profile, sel!(buttonA), 1.);
        assert_eq!(read(), (true, 0.), "a fresh press after letting go");
    });
}

#[test]
fn a_press_outliving_a_keyboard_no_read_saw_stays_hidden() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithMicroGamepad];
        let profile: ObjcId = msg_send![controller, microGamepad];
        set_button(profile, sel!(buttonA), 1.);
        set_keyboard_up(true);
        set_keyboard_up(false);
        let mut controllers = Controllers::default();
        read_controller(controller, ControllerId(0), &mut controllers);
        hide_keyboard_presses(&mut controllers);
        assert!(!controllers.micro_gamepads[0].is_pressed(MicroButton::Select));
    });
}

#[test]
fn a_stick_or_trigger_outliving_the_keyboard_stays_hidden_until_it_rests() {
    objc::rc::autoreleasepool(|| unsafe {
        let controller: ObjcId = msg_send![class!(GCController), controllerWithExtendedGamepad];
        let profile: ObjcId = msg_send![controller, extendedGamepad];
        let read = || {
            let mut controllers = Controllers::default();
            read_controller(controller, ControllerId(0), &mut controllers);
            hide_keyboard_presses(&mut controllers);
            let pad = &controllers.gamepads[0];
            (pad.left_stick.x, pad.left_trigger)
        };
        set_direction(profile, sel!(leftThumbstick), 0.75, 0.);
        set_keyboard_up(true);
        set_keyboard_up(false);
        assert_eq!(read(), (0., 0.), "the stick is still pushed");

        set_direction(profile, sel!(leftThumbstick), 0.15, 0.15);
        assert_eq!(read(), (0., 0.), "and diagonally, it counts its full reach");

        set_direction(profile, sel!(leftThumbstick), 0., 0.);
        set_button(profile, sel!(leftTrigger), 0.5);
        assert_eq!(read(), (0., 0.), "the trigger is still pulled");

        set_button(profile, sel!(leftTrigger), 0.);
        assert_eq!(read(), (0., 0.));
        set_direction(profile, sel!(leftThumbstick), 0.5, 0.);
        assert_eq!(read(), (0.5, 0.), "a fresh push once both rest");
    });
}
