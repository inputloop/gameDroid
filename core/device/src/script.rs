// SPDX-License-Identifier: GPL-3.0-or-later
//! Small input scripts for tests and demonstrations: wait, tap, swipe,
//! press a key, save the screen. One step per line.
//!
//! ```text
//! # wake the screen and swipe up to unlock
//! wait 5
//! swipe 360 1100 360 300 400
//! wait 3
//! screenshot unlocked.ppm
//! tap 360 640
//! key 102
//! ```
//!
//! Three more steps talk to Android's shell on the serial port:
//!
//! ```text
//! shell getprop ro.build.version.release
//! expect GD_READY 60          # wait up to 60 s for this text on the console
//! install                     # install the APK the runner was given
//! start com.example.game      # start an installed app
//! ```
//!
//! Coordinates are screen pixels, durations and waits are milliseconds for
//! `swipe` and seconds for `wait`, and key numbers are Linux key codes.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Wait(Duration),
    Tap {
        x: i32,
        y: i32,
    },
    Swipe {
        from: (i32, i32),
        to: (i32, i32),
        over: Duration,
    },
    Key(u16),
    Screenshot(PathBuf),
    /// Type a command line at Android's serial shell.
    Shell(String),
    /// Wait until the console has shown `text`, or fail after `within`.
    Expect {
        text: String,
        within: Duration,
    },
    /// Install the APK the runner was started with.
    Install,
    /// Start an installed app by package name.
    Start(String),
}

/// Parse a script. Errors name the line.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("script line {}: {why}", i + 1);
        let mut words = line.split_whitespace();
        let verb = words.next().unwrap_or("");
        let rest: Vec<&str> = words.collect();
        let num = |n: usize| -> Result<i32, String> {
            rest.get(n)
                .and_then(|w| w.parse::<i32>().ok())
                .filter(|v| *v >= 0)
                .ok_or_else(|| bad("expected a non-negative whole number"))
        };
        let count = |want: usize| {
            if rest.len() == want {
                Ok(())
            } else {
                Err(bad(&format!("`{verb}` takes {want} value(s)")))
            }
        };
        steps.push(match verb {
            "wait" => {
                count(1)?;
                let secs: f64 = rest[0]
                    .parse()
                    .ok()
                    .filter(|s: &f64| s.is_finite() && *s >= 0.0 && *s <= 3600.0)
                    .ok_or_else(|| bad("expected seconds, 0 to 3600"))?;
                Step::Wait(Duration::from_secs_f64(secs))
            }
            "tap" => {
                count(2)?;
                Step::Tap {
                    x: num(0)?,
                    y: num(1)?,
                }
            }
            "swipe" => {
                count(5)?;
                Step::Swipe {
                    from: (num(0)?, num(1)?),
                    to: (num(2)?, num(3)?),
                    over: Duration::from_millis(num(4)? as u64),
                }
            }
            "key" => {
                count(1)?;
                Step::Key(u16::try_from(num(0)?).map_err(|_| bad("key code out of range"))?)
            }
            "screenshot" => {
                count(1)?;
                Step::Screenshot(rest[0].into())
            }
            "shell" => {
                if rest.is_empty() {
                    return Err(bad("`shell` needs a command"));
                }
                Step::Shell(rest.join(" "))
            }
            "expect" => {
                count(2)?;
                Step::Expect {
                    text: rest[0].to_string(),
                    within: Duration::from_secs(num(1)? as u64),
                }
            }
            "install" => {
                count(0)?;
                Step::Install
            }
            "start" => {
                count(1)?;
                let ok = rest[0]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
                if !ok {
                    return Err(bad("that is not a package name"));
                }
                Step::Start(rest[0].to_string())
            }
            other => return Err(bad(&format!("unknown step `{other}`"))),
        });
    }
    Ok(steps)
}

/// The points a finger passes through on a swipe: both ends and evenly
/// spaced points between, about one every 16 ms.
pub fn swipe_points(from: (i32, i32), to: (i32, i32), over: Duration) -> Vec<(i32, i32)> {
    let steps = (over.as_millis() / 16).clamp(1, 600) as i32;
    (0..=steps)
        .map(|i| {
            (
                from.0 + (to.0 - from.0) * i / steps,
                from.1 + (to.1 - from.1) * i / steps,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_step() {
        let steps = parse(
            "# unlock\nwait 2.5\n\nswipe 360 1100 360 300 400  # up\ntap 10 20\nkey 102\nscreenshot out/a.ppm\n",
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                Step::Wait(Duration::from_millis(2500)),
                Step::Swipe {
                    from: (360, 1100),
                    to: (360, 300),
                    over: Duration::from_millis(400)
                },
                Step::Tap { x: 10, y: 20 },
                Step::Key(102),
                Step::Screenshot("out/a.ppm".into()),
            ]
        );
        assert!(parse("").unwrap().is_empty());
        assert_eq!(
            parse("shell ls -l /data\nexpect GD_OK 30\ninstall\nstart org.example.game_1").unwrap(),
            [
                Step::Shell("ls -l /data".into()),
                Step::Expect {
                    text: "GD_OK".into(),
                    within: Duration::from_secs(30)
                },
                Step::Install,
                Step::Start("org.example.game_1".into()),
            ]
        );
    }

    #[test]
    fn errors_name_the_line() {
        for (text, want) in [
            ("wait 1\njump 3", "line 2: unknown step `jump`"),
            ("tap 5", "line 1: `tap` takes 2"),
            ("tap -5 3", "line 1: expected a non-negative"),
            ("wait forever", "line 1: expected seconds"),
            ("key 70000", "line 1: key code out of range"),
            ("swipe 1 2 3 4", "line 1: `swipe` takes 5"),
            ("shell", "line 1: `shell` needs a command"),
            ("install now", "line 1: `install` takes 0"),
            ("start com.x;reboot", "line 1: that is not a package name"),
            ("expect DONE soon", "line 1: expected a non-negative"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(want), "{text:?}: {e}");
        }
    }

    #[test]
    fn a_swipe_starts_and_ends_exactly_where_asked() {
        let p = swipe_points((360, 1100), (360, 300), Duration::from_millis(400));
        assert_eq!(p.len(), 26);
        assert_eq!((p[0], p[25]), ((360, 1100), (360, 300)));
        assert!(p.windows(2).all(|w| w[1].1 < w[0].1), "moves steadily up");
        let quick = swipe_points((0, 0), (10, 10), Duration::ZERO);
        assert_eq!(quick, [(0, 0), (10, 10)]);
    }
}
