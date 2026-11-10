// SPDX-License-Identifier: GPL-3.0-or-later
//! gameDroid: Android in a window.
//!
//! Shows the guest's screen, and sends it the mouse as a finger and the
//! keyboard as typing, or, with a game profile loaded and game mode on
//! (F1), keys and mouse as the touches the profile describes.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gamedroid::frontend::{Action, Frontend, HostState, Mode};
use gamedroid::view::rgb_to_window;
use gamedroid_device::android::{machine, true_colours, Handles, Options};
use gamedroid_input::{Profile, Touch};
use minifb::{MouseButton, MouseMode, ScaleMode, Window, WindowOptions};

const USAGE: &str = "\
usage: gamedroid <prepared-dir> [options]
       gamedroid --demo [options]

  <prepared-dir>      an Android image set made by tools/engine/android_prepare.py
  --demo              no Android: a test card that shows the touches it receives
  --profile <file>    a game profile (TOML); F1 then switches game mode on and off
  --game-mode         start in game mode (needs --profile)
  --cpus <n>          virtual processors (default 2)
  --memory <MiB>      guest memory (default 4096)
  --console <file>    write Android's console log here (default: discard)
  --frames <n>        exit after n frames (for tests)

in the window:
  mouse        a finger (direct mode)
  keyboard     types into Android (direct mode)
  F1           game mode on or off: keys and mouse follow the profile
  Esc          Back        F2    Home";

const DEMO_SIZE: (u32, u32) = (720, 1280);

/// Where frames come from and input goes to.
enum Guest {
    Android(Handles),
    Demo(Demo),
}

/// A stand-in for Android that draws the touches and keys it is sent.
struct Demo {
    rgb: Vec<u8>,
    fingers: [Option<(i32, i32)>; 10],
    keys: Vec<u16>,
    dirty: bool,
}

impl Demo {
    fn new() -> Demo {
        Demo {
            rgb: Vec::new(),
            fingers: [None; 10],
            keys: Vec::new(),
            dirty: true,
        }
    }

    fn render(&mut self) {
        let (w, h) = (DEMO_SIZE.0 as usize, DEMO_SIZE.1 as usize);
        self.rgb.clear();
        self.rgb.reserve(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                // A grid every 80 pixels on a dark background, thick enough
                // to survive being scaled down.
                let line = x % 80 < 3 || y % 80 < 3;
                self.rgb.extend_from_slice(if line {
                    &[0x30, 0x3a, 0x4a]
                } else {
                    &[0x12, 0x16, 0x1e]
                });
            }
        }
        const COLOURS: [[u8; 3]; 10] = [
            [0xff, 0x5c, 0x5c],
            [0x5c, 0xc8, 0xff],
            [0x7d, 0xff, 0x7a],
            [0xff, 0xd7, 0x5c],
            [0xd0, 0x8c, 0xff],
            [0xff, 0x9a, 0x3d],
            [0x5c, 0xff, 0xe0],
            [0xff, 0x7a, 0xc8],
            [0xc8, 0xc8, 0xc8],
            [0xff, 0xff, 0xff],
        ];
        for (i, f) in self.fingers.iter().enumerate() {
            let Some((fx, fy)) = f else { continue };
            for y in (fy - 28).max(0)..(fy + 28).min(h as i32) {
                for x in (fx - 28).max(0)..(fx + 28).min(w as i32) {
                    let at = (y as usize * w + x as usize) * 3;
                    self.rgb[at..at + 3].copy_from_slice(&COLOURS[i]);
                }
            }
        }
        // One bar along the top per key held, so typing is visible too.
        for (n, _) in self.keys.iter().enumerate().take(16) {
            for y in 8..32 {
                for x in (8 + n * 44)..(8 + n * 44 + 36) {
                    let at = (y * w + x) * 3;
                    self.rgb[at..at + 3].copy_from_slice(&[0xe0, 0xe0, 0x40]);
                }
            }
        }
        self.dirty = false;
    }
}

impl Guest {
    fn size(&self) -> (u32, u32) {
        match self {
            Guest::Android(h) => h.display.size(),
            Guest::Demo(_) => DEMO_SIZE,
        }
    }

    fn apply(&mut self, action: Action, fingers_down: &mut [bool; 10]) {
        if let Action::Touch(t) = action {
            match t {
                Touch::Down { finger, .. } | Touch::Move { finger, .. } => {
                    fingers_down[finger as usize % 10] = true
                }
                Touch::Up { finger } => fingers_down[finger as usize % 10] = false,
            }
        }
        match self {
            Guest::Android(h) => match action {
                Action::Touch(Touch::Down { finger, x, y })
                | Action::Touch(Touch::Move { finger, x, y }) => h.touch.touch(finger, x, y),
                Action::Touch(Touch::Up { finger }) => {
                    h.touch.release(finger, fingers_down.iter().all(|d| !*d))
                }
                Action::Key { code, down } => h.keyboard.key(code, down),
            },
            Guest::Demo(d) => {
                match action {
                    Action::Touch(Touch::Down { finger, x, y })
                    | Action::Touch(Touch::Move { finger, x, y }) => {
                        d.fingers[finger as usize % 10] = Some((x, y))
                    }
                    Action::Touch(Touch::Up { finger }) => d.fingers[finger as usize % 10] = None,
                    Action::Key { code, down: true } => d.keys.push(code),
                    Action::Key { code, down: false } => d.keys.retain(|k| *k != code),
                }
                d.dirty = true;
            }
        }
    }

    /// The newest picture as RGB, if it changed since `seen`.
    fn frame(&mut self, seen: &mut u64) -> Option<((u32, u32), Vec<u8>)> {
        match self {
            Guest::Android(h) => {
                if h.display.sequence() == *seen {
                    return None;
                }
                let frame = true_colours(h.display.frame());
                *seen = frame.sequence;
                Some(((frame.width, frame.height), frame.to_rgb()))
            }
            Guest::Demo(d) => {
                if !d.dirty {
                    return None;
                }
                d.render();
                Some((DEMO_SIZE, d.rgb.clone()))
            }
        }
    }
}

struct Args {
    dir: Option<PathBuf>,
    demo: bool,
    profile: Option<PathBuf>,
    game_mode: bool,
    options: Options,
    console: Option<PathBuf>,
    frames: Option<u64>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        dir: None,
        demo: false,
        profile: None,
        game_mode: false,
        options: Options::default(),
        console: None,
        frames: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| args.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--demo" => a.demo = true,
            "--game-mode" => a.game_mode = true,
            "--profile" => a.profile = Some(value(&arg)?.into()),
            "--console" => a.console = Some(value(&arg)?.into()),
            "--cpus" => {
                a.options.cpus = value(&arg)?.parse().map_err(|_| "--cpus needs a number")?
            }
            "--memory" => {
                a.options.memory_mib = value(&arg)?
                    .parse()
                    .map_err(|_| "--memory needs a number")?
            }
            "--frames" => {
                a.frames = Some(
                    value(&arg)?
                        .parse()
                        .map_err(|_| "--frames needs a number")?,
                )
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            dir => a.dir = Some(dir.into()),
        }
    }
    if a.demo == a.dir.is_some() {
        return Err("give a prepared image folder, or --demo".into());
    }
    if a.game_mode && a.profile.is_none() {
        return Err("--game-mode needs --profile".into());
    }
    Ok(a)
}

fn title(mode: Mode, has_profile: bool, name: &str) -> String {
    match (mode, has_profile) {
        (Mode::Game, _) => format!("gameDroid - game mode: {name} (F1: direct)"),
        (Mode::Direct, true) => format!("gameDroid - direct (F1: game mode, {name})"),
        (Mode::Direct, false) => "gameDroid - direct".to_string(),
    }
}

fn run() -> Result<(), String> {
    let args = parse()?;
    let profile = match &args.profile {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            Some(Profile::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    let name = profile.as_ref().map(|p| p.name.clone()).unwrap_or_default();

    let mut guest = match &args.dir {
        Some(dir) => {
            let (config, handles) = machine(dir, &args.options)?;
            let console: Box<dyn std::io::Write + Send> = match &args.console {
                Some(path) => Box::new(
                    std::fs::File::create(path)
                        .map_err(|e| format!("creating {}: {e}", path.display()))?,
                ),
                None => Box::new(std::io::sink()),
            };
            std::thread::spawn(move || {
                match bvmm::run_linux(&config, console, None) {
                    Ok(_) => eprintln!("gamedroid: Android stopped"),
                    Err(e) => eprintln!("gamedroid: {e}"),
                }
                std::process::exit(1);
            });
            Guest::Android(handles)
        }
        None => Guest::Demo(Demo::new()),
    };

    let guest_size = guest.size();
    let mut frontend = Frontend::new(guest_size, profile.as_ref());
    // A window with the guest's shape, small enough for most screens.
    let scale = 800.0 / guest_size.1.max(1) as f32;
    let window_size = (
        (guest_size.0 as f32 * scale).round().max(160.0) as usize,
        (guest_size.1 as f32 * scale).round().max(160.0) as usize,
    );
    let mut window = Window::new(
        &title(frontend.mode(), frontend.has_profile(), &name),
        window_size.0,
        window_size.1,
        WindowOptions {
            resize: true,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .map_err(|e| format!("opening the window: {e}"))?;
    window.set_target_fps(60);

    let mut pixels: Vec<u32> = vec![0; guest_size.0 as usize * guest_size.1 as usize];
    let mut picture = guest_size;
    let mut seen = 0u64;
    let mut fingers_down = [false; 10];
    let mut mode = frontend.mode();
    let mut frames = 0u64;
    let mut want_game_mode = args.game_mode;
    let started = Instant::now();

    while window.is_open() {
        let mut state = HostState {
            keys: window.get_keys(),
            mouse: window.get_mouse_pos(MouseMode::Discard),
            buttons: [
                window.get_mouse_down(MouseButton::Left),
                window.get_mouse_down(MouseButton::Right),
                window.get_mouse_down(MouseButton::Middle),
            ],
            window: window.get_size(),
        };
        // A key tapped and let go between two frames is not in the held
        // list any more; count it as held for this one frame so it is seen.
        for key in window.get_keys_pressed(minifb::KeyRepeat::No) {
            if !state.keys.contains(&key) {
                state.keys.push(key);
            }
        }
        if std::mem::take(&mut want_game_mode) {
            state.keys.push(minifb::Key::F1);
        }
        for action in frontend.step(&state) {
            guest.apply(action, &mut fingers_down);
        }
        if frontend.mode() != mode {
            mode = frontend.mode();
            window.set_title(&title(mode, frontend.has_profile(), &name));
        }
        if let Some((size, rgb)) = guest.frame(&mut seen) {
            picture = size;
            rgb_to_window(&rgb, &mut pixels);
        }
        window
            .update_with_buffer(&pixels, picture.0 as usize, picture.1 as usize)
            .map_err(|e| format!("drawing: {e}"))?;
        frames += 1;
        if args.frames.is_some_and(|n| frames >= n) {
            eprintln!(
                "gamedroid: {frames} frames in {:.1} s",
                started.elapsed().as_secs_f32()
            );
            break;
        }
        // minifb paces the loop; this only guards against a busy spin if it does not.
        if started.elapsed() < Duration::from_millis(frames) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gamedroid: {e}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
