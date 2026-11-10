// SPDX-License-Identifier: GPL-3.0-or-later
//! `gamedroid-run`: start Android from a prepared image folder, with no
//! window. The guest's console goes to standard output and the newest frame
//! is kept in a PPM file. For bring-up and CI; the real launcher comes later.

use std::path::PathBuf;
use std::process::ExitCode;

use gamedroid_device::android::{machine, Options};

const USAGE: &str = "\
usage: gamedroid-run <prepared-dir> [options]

  <prepared-dir>        made by tools/engine/android_prepare.py
  --cpus <n>            virtual processors (default 2)
  --memory <MiB>        guest memory (default 4096)
  --display <W>x<H>     screen size (default 720x1280)
  --screenshot <file>   keep the newest frame here as a PPM, about once a second
  --cmdline <text>      extra kernel arguments";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next().filter(|a| !a.starts_with("--")) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let mut options = Options::default();
    let mut screenshot: Option<PathBuf> = None;
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
                let frame = display.frame();
                saved = frame.sequence;
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, frame.to_ppm()).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        });
    }
    match bvmm::run_linux(&config, Box::new(std::io::stdout()), None) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gamedroid-run: {e}");
            ExitCode::FAILURE
        }
    }
}
