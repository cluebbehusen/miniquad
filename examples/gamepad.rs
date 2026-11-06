//! Logs every connected controller's input: connections, buttons,
//! sticks, triggers and the Siri Remote's touch surface.

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
mod apple {
    use miniquad::{
        gamepad::{self, Button, Controllers, Direction, Gamepad, MicroButton, MicroGamepad},
        *,
    };

    struct Stage {
        ctx: Box<dyn RenderingBackend>,
        last: Controllers,
    }

    impl EventHandler for Stage {
        fn update(&mut self) {
            let now = gamepad::controllers();
            log_gamepads(&self.last.gamepads, &now.gamepads);
            log_micro_gamepads(&self.last.micro_gamepads, &now.micro_gamepads);
            self.last = now;
        }

        fn draw(&mut self) {
            self.ctx
                .begin_default_pass(PassAction::clear_color(0.1, 0.1, 0.1, 1.));
            self.ctx.end_render_pass();
            self.ctx.commit_frame();
        }
    }

    /// Analog values in tenths, so the log shows movement without noise.
    fn tenths(value: f32) -> i32 {
        (value * 10.).round() as i32
    }

    fn direction_moved(before: Direction, after: Direction) -> bool {
        tenths(before.x) != tenths(after.x) || tenths(before.y) != tenths(after.y)
    }

    fn log_gamepads(before: &[Gamepad], after: &[Gamepad]) {
        for pad in before {
            if !after.iter().any(|now| now.id == pad.id) {
                println!("{:?} disconnected", pad.id);
            }
        }
        for pad in after {
            let Some(previous) = before.iter().find(|then| then.id == pad.id) else {
                let missing: Vec<Button> = Button::ALL
                    .iter()
                    .copied()
                    .filter(|&button| !pad.has(button))
                    .collect();
                println!(
                    "{:?} connected: gamepad, {:?}, {:?}, missing {:?}",
                    pad.id, pad.family, pad.name, missing
                );
                continue;
            };
            for button in Button::ALL {
                if pad.is_pressed(button) != previous.is_pressed(button) {
                    let state = if pad.is_pressed(button) { "down" } else { "up" };
                    println!("{:?} {:?} {}", pad.id, button, state);
                }
            }
            if direction_moved(previous.left_stick, pad.left_stick) {
                println!("{:?} left stick {:?}", pad.id, pad.left_stick);
            }
            if direction_moved(previous.right_stick, pad.right_stick) {
                println!("{:?} right stick {:?}", pad.id, pad.right_stick);
            }
            if tenths(previous.left_trigger) != tenths(pad.left_trigger) {
                println!("{:?} left trigger {:.2}", pad.id, pad.left_trigger);
            }
            if tenths(previous.right_trigger) != tenths(pad.right_trigger) {
                println!("{:?} right trigger {:.2}", pad.id, pad.right_trigger);
            }
        }
    }

    fn log_micro_gamepads(before: &[MicroGamepad], after: &[MicroGamepad]) {
        for pad in before {
            if !after.iter().any(|now| now.id == pad.id) {
                println!("{:?} disconnected", pad.id);
            }
        }
        for pad in after {
            let Some(previous) = before.iter().find(|then| then.id == pad.id) else {
                println!("{:?} connected: micro gamepad, {:?}", pad.id, pad.name);
                continue;
            };
            for button in MicroButton::ALL {
                if pad.is_pressed(button) != previous.is_pressed(button) {
                    let state = if pad.is_pressed(button) { "down" } else { "up" };
                    println!("{:?} {:?} {}", pad.id, button, state);
                }
            }
            if direction_moved(previous.touch, pad.touch) {
                println!("{:?} touch {:?}", pad.id, pad.touch);
            }
        }
    }

    pub fn main() {
        println!("Reading controllers; connect one or press its buttons.");
        miniquad::start(
            conf::Conf {
                window_title: "Gamepad".to_string(),
                ..Default::default()
            },
            || {
                Box::new(Stage {
                    ctx: window::new_rendering_backend(),
                    last: Controllers::default(),
                })
            },
        );
    }
}

fn main() {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "tvos"))]
    apple::main();

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "tvos")))]
    println!("Controllers are read on macOS, iOS and tvOS only.");
}
