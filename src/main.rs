use std::{collections::HashMap, fs::File, io::Read, sync::Arc};

use audio::{Player, SquareWave};
use clap::Parser;
use hardware::Screen;
use interpreter::{Chip8Interpreter, Config};
use pixels::{wgpu::TextureFormat, Pixels, PixelsBuilder, SurfaceTexture};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

mod audio;
mod hardware;
mod interpreter;

#[derive(clap::Parser)]
#[command(long_about = None)]
struct Args {
    /// Path to rom
    rom: String,

    /// Whether to use the "original" shift behavior. That is, if this is set,
    /// then SHR VX, VY actually does vx = vy >> 1. Otherwise, it does vx = vx >> 1.
    #[arg(long)]
    original_shift_behavior: bool,

    /// If this is set, the address pointer I is incremented on certain LD instructions.
    #[arg(long)]
    original_addressing_behavior: bool,
}

fn main() {
    let args = Args::parse();

    let mut prog = Vec::new();
    File::open(args.rom)
        .unwrap()
        .read_to_end(&mut prog)
        .unwrap();

    let config = Config {
        use_original_address_register_behavior: args.original_addressing_behavior,
        use_original_shift_semantics: args.original_shift_behavior,
        ..Default::default()
    };

    run(&prog, config)
}

pub fn run(rom: &[u8], config: Config) {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::with_config(config);

    app.interpreter.load_program(rom);

    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    event_loop.run_app(&mut app).unwrap();
}

struct WindowState {
    window: Arc<Window>,
    pixels: Pixels<'static>,
}

impl WindowState {
    fn create(event_loop: &ActiveEventLoop) -> Self {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();

        window.set_title("Chip8 Emulator");

        let window = Arc::new(window);

        let surface =
            SurfaceTexture::new(Screen::WIDTH as u32, Screen::HEIGHT as u32, window.clone());

        let pixels = PixelsBuilder::new(Screen::WIDTH as u32, Screen::HEIGHT as u32, surface)
            .enable_vsync(true)
            .texture_format(TextureFormat::Rgba8UnormSrgb)
            .build()
            .unwrap();

        Self { window, pixels }
    }
}

fn default_keymap() -> HashMap<PhysicalKey, u8> {
    // Keys look like this
    // 1 2 3 c
    // 4 5 6 d
    // 7 8 9 e
    // a 0 b f

    [
        // Row 1
        (PhysicalKey::Code(KeyCode::Digit1), 0x1),
        (PhysicalKey::Code(KeyCode::Digit2), 0x2),
        (PhysicalKey::Code(KeyCode::Digit3), 0x3),
        (PhysicalKey::Code(KeyCode::Digit4), 0xc),
        // Row 2
        (PhysicalKey::Code(KeyCode::KeyQ), 0x4),
        (PhysicalKey::Code(KeyCode::KeyW), 0x5),
        (PhysicalKey::Code(KeyCode::KeyE), 0x6),
        (PhysicalKey::Code(KeyCode::KeyR), 0xd),
        // Row 3
        (PhysicalKey::Code(KeyCode::KeyA), 0x7),
        (PhysicalKey::Code(KeyCode::KeyS), 0x8),
        (PhysicalKey::Code(KeyCode::KeyD), 0x9),
        (PhysicalKey::Code(KeyCode::KeyF), 0xe),
        // Row 4
        (PhysicalKey::Code(KeyCode::KeyZ), 0xa),
        (PhysicalKey::Code(KeyCode::KeyX), 0x0),
        (PhysicalKey::Code(KeyCode::KeyC), 0xb),
        (PhysicalKey::Code(KeyCode::KeyV), 0xf),
    ]
    .into_iter()
    .collect()
}

struct App {
    window_state: Option<WindowState>,
    keymap: HashMap<PhysicalKey, u8>,
    interpreter: Chip8Interpreter,
    player: Player,
}

impl App {
    pub fn with_config(config: Config) -> Self {
        let keymap: HashMap<_, u8> = default_keymap();

        let player = Player::new(SquareWave::new(440));

        Self {
            window_state: None,
            keymap,
            interpreter: Chip8Interpreter::with_config(config),
            player,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.window_state = Some(WindowState::create(event_loop));
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.window_state.as_ref() {
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Window events imply the existence of a window
        let WindowState { window, pixels } = self.window_state.as_mut().unwrap();

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                if size.height == 0 || size.width == 0 {
                    return;
                }

                let frame = pixels.frame_mut();
                self.interpreter.tick(frame);

                self.player.set(self.interpreter.should_play_sound());

                // This is where vsync comes in, i.e. where the graphics pipeline somehow
                // blocks our progress and makes us wait so we don't just busy-loop
                // and max out the CPU
                pixels.render().unwrap();
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = self.keymap.get(&event.physical_key) {
                    match event.state {
                        ElementState::Pressed => self.interpreter.set_key(*key, true),
                        ElementState::Released => self.interpreter.set_key(*key, false),
                    }
                }

                if event.physical_key == PhysicalKey::Code(KeyCode::Space)
                    && event.state == ElementState::Pressed
                {
                    self.interpreter.unhalt();
                }
            }
            WindowEvent::Resized(size) => {
                let (min_w, min_h) = self.interpreter.screen_size();
                let new_width = (size.width).max(min_w);
                let new_height = (size.height).max(min_h);

                pixels.resize_surface(new_width, new_height).unwrap();
            }
            _ => {}
        }
    }
}
