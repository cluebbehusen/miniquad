//! Prints the display's refresh rate and the measured frame rate once a
//! second. Space cycles the frame interval through 1 to 4; a number given
//! on the command line sets the starting interval.

use miniquad::*;

struct Stage {
    ctx: Box<dyn RenderingBackend>,
    interval: u32,
    frame: u64,
    window_start: f64,
    window_frames: u32,
}

impl Stage {
    fn set_interval(&mut self, interval: u32) {
        self.interval = interval;
        window::set_frame_interval(interval);
        self.window_start = date::now();
        self.window_frames = 0;
    }
}

impl EventHandler for Stage {
    fn update(&mut self) {}

    fn draw(&mut self) {
        self.frame += 1;
        self.window_frames += 1;
        let elapsed = date::now() - self.window_start;
        if elapsed >= 1. {
            println!(
                "refresh {:?} Hz, interval {}, {:.1} fps",
                window::refresh_rate(),
                self.interval,
                self.window_frames as f64 / elapsed
            );
            self.window_start = date::now();
            self.window_frames = 0;
        }

        let shade = (self.frame % 60) as f32 / 60.;
        self.ctx
            .begin_default_pass(PassAction::clear_color(shade, 0.2, 1. - shade, 1.));
        self.ctx.end_render_pass();
        self.ctx.commit_frame();
    }

    fn key_down_event(&mut self, keycode: KeyCode, _: KeyMods, repeat: bool) {
        if keycode == KeyCode::Space && !repeat {
            self.set_interval(self.interval % 4 + 1);
        }
    }
}

fn main() {
    let interval = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1);
    miniquad::start(conf::Conf::default(), move || {
        let mut stage = Stage {
            ctx: window::new_rendering_backend(),
            interval,
            frame: 0,
            window_start: date::now(),
            window_frames: 0,
        };
        stage.set_interval(interval);
        Box::new(stage)
    });
}
