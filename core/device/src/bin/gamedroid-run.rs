// SPDX-License-Identifier: GPL-3.0-or-later
//! `gamedroid-run`: start Android from a prepared image folder, with no
//! window. The guest's console goes to standard output and the newest frame
//! is kept in a PPM file. For bring-up and CI; the real launcher comes later.

use std::path::PathBuf;
use std::process::ExitCode;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gamedroid_device::android::{file_as_disk, machine, true_colours, Handles, Options};
use gamedroid_device::script::{self, Step};

/// What Android's init logs when the system has finished booting.
const BOOTED: &[u8] = b"sys.boot_completed=1";

/// Passes the guest console through, notices when Android has booted, and
/// keeps the recent text so a script can wait for something to appear.
struct Console {
    out: std::io::Stdout,
    line: Vec<u8>,
    booted: Arc<AtomicBool>,
    recent: Arc<Mutex<String>>,
}

/// How much console text is kept for `expect`.
const RECENT: usize = 64 << 10;

impl Write for Console {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for b in bytes {
            if *b == b'\n' {
                if self.line.windows(BOOTED.len()).any(|w| w == BOOTED) {
                    self.booted.store(true, Ordering::SeqCst);
                }
                let mut recent = self.recent.lock().unwrap();
                recent.push_str(&String::from_utf8_lossy(&self.line));
                recent.push('\n');
                if recent.len() > RECENT {
                    let mut cut = recent.len() - RECENT / 2;
                    while !recent.is_char_boundary(cut) {
                        cut += 1;
                    }
                    recent.drain(..cut);
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

/// The guest's serial input: whatever is sent down the channel.
struct Keys {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for Keys {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

/// What a script needs besides the device handles.
struct Session {
    handles: Handles,
    booted: Arc<AtomicBool>,
    recent: Arc<Mutex<String>>,
    serial: Sender<Vec<u8>>,
    /// True length of the APK on the extra disk, if one was given.
    apk_bytes: Option<u64>,
}

impl Session {
    /// Type one command line at Android's shell.
    fn shell(&self, command: &str) {
        let _ = self.serial.send(format!("{command}\n").into_bytes());
    }

    /// Wait for `text` on the console. False if it does not appear in time.
    fn expect(&self, text: &str, within: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.recent.lock().unwrap().contains(text) {
                return true;
            }
            if start.elapsed() > within {
                return false;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Wait for boot, run the script, then end the process. The exit status is
/// 0 if every `expect` was met and 3 if one was not.
fn run_script(steps: Vec<Step>, session: Session) {
    std::thread::spawn(move || {
        while !session.booted.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
        eprintln!("gamedroid-run: Android booted; running the script");
        let handles = &session.handles;
        let mut failed = false;
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
                Step::Shell(command) => session.shell(&command),
                Step::Expect { text, within } => {
                    if session.expect(&text, within) {
                        eprintln!("gamedroid-run: saw `{text}`");
                    } else {
                        eprintln!(
                            "gamedroid-run: EXPECT FAILED: `{text}` did not appear within {} s",
                            within.as_secs()
                        );
                        failed = true;
                    }
                }
                Step::Install => match session.apk_bytes {
                    None => {
                        eprintln!("gamedroid-run: EXPECT FAILED: `install` needs --apk");
                        failed = true;
                    }
                    Some(bytes) => {
                        // The APK is the start of the second disk. Copy
                        // exactly its length out as root, then install it.
                        // The marker is split in the command so that the
                        // shell's echo of what was typed does not match it.
                        session.shell(&format!(
                            "su 0 sh -c 'head -c {bytes} /dev/block/vdb > /data/local/tmp/gd.apk; \
                             chmod 644 /data/local/tmp/gd.apk'; \
                             pm install -r /data/local/tmp/gd.apk; echo GD_INSTALL''_STATUS=$?"
                        ));
                        if session.expect("GD_INSTALL_STATUS=0", Duration::from_secs(180)) {
                            eprintln!("gamedroid-run: APK installed ({bytes} bytes)");
                        } else {
                            eprintln!("gamedroid-run: EXPECT FAILED: the APK did not install");
                            failed = true;
                        }
                    }
                },
                // Ask the package manager for the app's launcher activity,
                // then start that. (`monkey`, the usual shortcut, aborts on
                // this image.)
                Step::Start(package) => session.shell(&format!(
                    "am start -n \"$(cmd package resolve-activity --brief \
                     -c android.intent.category.LAUNCHER {package} | tail -n 1)\"; \
                     echo GD_START''_STATUS=$?"
                )),
            }
        }
        eprintln!("gamedroid-run: script finished");
        std::process::exit(if failed { 3 } else { 0 });
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
  --apk <file>          hand this APK to Android; the script's `install` step installs it
  --script <file>       script to run once Android has booted (see
                        core/device/src/script.rs); the run ends when it does,
                        with status 3 if an `expect` was not met";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next().filter(|a| !a.starts_with("--")) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let mut options = Options::default();
    let mut screenshot: Option<PathBuf> = None;
    let mut steps: Option<Vec<Step>> = None;
    let mut apk: Option<PathBuf> = None;
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
            "--apk" => {
                apk = Some(value.into());
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

    let dir = PathBuf::from(dir);
    let mut apk_bytes = None;
    if let Some(apk) = &apk {
        let layout = dir.join("apk.layout");
        match file_as_disk(apk, &layout) {
            Ok(bytes) => {
                apk_bytes = Some(bytes);
                options.extra_disk = Some(layout);
            }
            Err(e) => {
                eprintln!("gamedroid-run: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (config, handles) = match machine(dir.as_path(), &options) {
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
    let recent = Arc::new(Mutex::new(String::new()));
    let console = Console {
        out: std::io::stdout(),
        line: Vec::new(),
        booted: booted.clone(),
        recent: recent.clone(),
    };
    let (serial, rx) = mpsc::channel();
    let keys = Keys {
        rx,
        pending: Vec::new(),
    };
    if let Some(steps) = steps {
        run_script(
            steps,
            Session {
                handles,
                booted,
                recent,
                serial,
                apk_bytes,
            },
        );
    } else {
        // Keep the serial line open for the whole run.
        std::mem::forget(serial);
    }
    match bvmm::run_linux(&config, Box::new(console), Some(Box::new(keys))) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gamedroid-run: {e}");
            ExitCode::FAILURE
        }
    }
}
