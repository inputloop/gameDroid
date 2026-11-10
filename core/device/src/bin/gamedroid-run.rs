// SPDX-License-Identifier: GPL-3.0-or-later
//! `gamedroid-run`: start Android from a prepared image folder, with no
//! window. The guest's console goes to standard output and the newest frame
//! is kept in a PPM file. For bring-up and CI; the real launcher comes later.

use std::path::PathBuf;
use std::process::ExitCode;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gamedroid_device::android::{machine, true_colours, Handles, Options};
use gamedroid_device::script::{self, Step};

/// What Android's init logs when the system has finished booting.
const BOOTED: &[u8] = b"sys.boot_completed=1";

/// Passes the guest console through and notices when Android has booted.
struct Console {
    out: std::io::Stdout,
    line: Vec<u8>,
    booted: Arc<AtomicBool>,
}

impl Write for Console {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for b in bytes {
            if *b == b'\n' {
                if self.line.windows(BOOTED.len()).any(|w| w == BOOTED) {
                    self.booted.store(true, Ordering::SeqCst);
                }
                self.line.clear();
            } else if self.line.len() < 4096 {
                self.line.push(*b);
            }
        }
        self.out.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// Wait for boot, run the script, then end the process.
fn run_script(steps: Vec<Step>, handles: Handles, booted: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !booted.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
        eprintln!("gamedroid-run: Android booted; running the script");
        for step in steps {
            match step {
                Step::Wait(d) => std::thread::sleep(d),
                Step::Tap { x, y } => {
                    handles.touch.touch(0, x, y);
                    std::thread::sleep(Duration::from_millis(60));
                    handles.touch.release(0, true);
                }
                Step::Swipe { from, to, over } => {
                    let points = script::swipe_points(from, to, over);
                    let pause = over / points.len() as u32;
                    for (x, y) in points {
                        handles.touch.touch(0, x, y);
                        std::thread::sleep(pause);
                    }
                    handles.touch.release(0, true);
                }
                Step::Key(code) => {
                    handles.keyboard.key(code, true);
                    std::thread::sleep(Duration::from_millis(60));
                    handles.keyboard.key(code, false);
                }
                Step::Screenshot(path) => {
                    let frame = true_colours(handles.display.frame());
                    match std::fs::write(&path, frame.to_ppm()) {
                        Ok(()) => eprintln!(
                            "gamedroid-run: saved frame {} to {}",
                            frame.sequence,
                            path.display()
                        ),
                        Err(e) => eprintln!("gamedroid-run: saving {}: {e}", path.display()),
                    }
                }
            }
        }
        eprintln!("gamedroid-run: script finished");
        std::process::exit(0);
    });
}

const USAGE: &str = "\
usage: gamedroid-run <prepared-dir> [options]

  <prepared-dir>        made by tools/engine/android_prepare.py
  --cpus <n>            virtual processors (default 2)
  --memory <MiB>        guest memory (default 4096)
  --display <W>x<H>     screen size (default 720x1280)
  --screenshot <file>   keep the newest frame here as a PPM, about once a second
  --cmdline <text>      extra kernel arguments
  --script <file>       input script to run once Android has booted (see
                        core/device/src/script.rs); the run ends when it does";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next().filter(|a| !a.starts_with("--")) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let mut options = Options::default();
    let mut screenshot: Option<PathBuf> = None;
    let mut steps: Option<Vec<Step>> = None;
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            eprintln!("gamedroid-run: {flag} needs a value\n\n{USAGE}");
            return ExitCode::FAILURE;
        };
        let ok = match flag.as_str() {
            "--cpus" => value.parse().map(|v| options.cpus = v).is_ok(),
            "--memory" => value.parse().map(|v| options.memory_mib = v).is_ok(),
            "--display" => value
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .map(|(w, h)| (options.width, options.height) = (w, h))
                .is_some(),
            "--screenshot" => {
                screenshot = Some(value.into());
                true
            }
            "--cmdline" => {
                options.extra_cmdline = value;
                true
            }
            "--script" => match std::fs::read_to_string(&value)
                .map_err(|e| format!("reading {value}: {e}"))
                .and_then(|text| script::parse(&text))
            {
                Ok(parsed) => {
                    steps = Some(parsed);
                    true
                }
                Err(e) => {
                    eprintln!("gamedroid-run: {e}");
                    return ExitCode::FAILURE;
                }
            },
            _ => false,
        };
        if !ok {
            eprintln!("gamedroid-run: bad option {flag}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }

    let (config, handles) = match machine(PathBuf::from(dir).as_path(), &options) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("gamedroid-run: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(path) = screenshot {
        let display = handles.display.clone();
        std::thread::spawn(move || {
            let mut saved = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if display.sequence() == saved {
                    continue;
                }
                let frame = true_colours(display.frame());
                saved = frame.sequence;
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, frame.to_ppm()).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        });
    }
    let booted = Arc::new(AtomicBool::new(false));
    let console = Console {
        out: std::io::stdout(),
        line: Vec::new(),
        booted: booted.clone(),
    };
    if let Some(steps) = steps {
        run_script(steps, handles, booted);
    }
    match bvmm::run_linux(&config, Box::new(console), None) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gamedroid-run: {e}");
            ExitCode::FAILURE
        }
    }
}
