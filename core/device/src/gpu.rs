// SPDX-License-Identifier: GPL-3.0-or-later
//! Deciding whether Android may draw with the computer's graphics card.
//!
//! Drawing with the graphics card needs the gfxstream library and a
//! graphics driver that offers Vulkan. Whether both are there and work is
//! found out by trying, in a separate process: a renderer that cannot start
//! has been seen to hang or crash instead of reporting an error, and a
//! separate process can be abandoned.
//!
//! A second guard covers what the check cannot see. Before Android is
//! started with the graphics card a mark is left on disk, and it is taken
//! away when Android has booted. A mark still there at the next start means
//! the last attempt never got that far, and the graphics card is left alone
//! until someone asks for it by name.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The gfxstream library's file name on this system.
pub const LIBRARY: &str = if cfg!(windows) {
    "gfxstream_backend.dll"
} else {
    "libgfxstream_backend.so"
};

/// How long a check may take before it is given up on.
const CHECK_TIME: Duration = Duration::from_secs(40);

/// What the person asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wish {
    /// Use the graphics card if it can be shown to work.
    Auto,
    /// Use it, and say why not if it cannot be.
    On,
    /// Do not.
    Off,
}

/// Where gameDroid keeps its own copy of the library.
pub fn own_copy(data: &Path) -> PathBuf {
    data.join("gpu").join(LIBRARY)
}

/// Put the copy of the library that gameDroid carries inside itself where
/// it can be loaded from. Nothing is written if it is already there whole.
pub fn unpack(data: &Path, library: &[u8]) -> Result<PathBuf, String> {
    let to = own_copy(data);
    if std::fs::metadata(&to).is_ok_and(|m| m.len() == library.len() as u64) {
        return Ok(to);
    }
    let dir = to.parent().unwrap_or(data);
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let part = to.with_extension("part");
    std::fs::write(&part, library).map_err(|e| format!("writing {}: {e}", part.display()))?;
    std::fs::rename(&part, &to).map_err(|e| format!("finishing {}: {e}", to.display()))?;
    Ok(to)
}

/// The library to use: the one `GAMEDROID_GFXSTREAM` names, else one beside
/// the program, else gameDroid's own copy in its data folder.
pub fn find(data: &Path) -> Option<PathBuf> {
    let named = std::env::var_os("GAMEDROID_GFXSTREAM").map(PathBuf::from);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(LIBRARY)));
    [named, beside, Some(own_copy(data))]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
}

/// Start the renderer in this process and see that it offers Vulkan.
/// This is what the separate process runs; it may hang or crash.
pub fn check_here(library: &Path) -> Result<(), String> {
    let renderer = bvmm::gfxstream::Gfxstream::start(library, 720, 1280, Box::new(|_| {}))
        .map_err(|e| e.to_string())?;
    // Capability set 3 is gfxstream's Vulkan.
    let (_, size) = renderer.capset_info(3);
    if size == 0 {
        return Err("the renderer started but does not offer Vulkan".into());
    }
    renderer
        .context_create(1, "gamedroid-check", 3)
        .map_err(|e| e.to_string())?;
    renderer.context_destroy(1);
    Ok(())
}

/// Run `program check_arg library` and judge by how it ends. `program` is
/// gameDroid itself, which answers that argument with [`check_here`].
pub fn check(program: &Path, check_arg: &str, library: &Path) -> Result<(), String> {
    let mut child = std::process::Command::new(program)
        .arg(check_arg)
        .arg(library)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("the check could not be started: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                use std::io::Read;
                let mut said = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut said);
                }
                let said = said.lines().last().unwrap_or("").trim().to_string();
                return match (status.success(), said.is_empty()) {
                    (true, _) => Ok(()),
                    (false, false) => Err(said),
                    (false, true) => Err(format!("the renderer stopped unexpectedly ({status})")),
                };
            }
            Ok(None) if started.elapsed() > CHECK_TIME => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "the renderer did not start (often: no graphics driver that offers Vulkan)"
                        .into(),
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("the check could not be watched: {e}")),
        }
    }
}

fn mark(data: &Path) -> PathBuf {
    data.join("gpu").join("starting")
}

/// Note that Android is about to be started with the graphics card.
pub fn starting(data: &Path) {
    let mark = mark(data);
    if let Some(dir) = mark.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        mark,
        "Android was started with the graphics card and had not booted yet.\n",
    );
}

/// Note that Android has booted: the graphics card works here.
pub fn booted(data: &Path) {
    let _ = std::fs::remove_file(mark(data));
}

/// Decide. Returns the library to draw with, if the graphics card is to be
/// used, and a sentence saying what was decided and why.
///
/// `check` is asked whether a library works (see [`check`]).
pub fn decide(
    wish: Wish,
    data: &Path,
    found: Option<PathBuf>,
    check: &dyn Fn(&Path) -> Result<(), String>,
) -> (Option<PathBuf>, String) {
    let software = "Android will draw with the processor";
    if wish == Wish::Off {
        return (None, format!("{software}, as asked."));
    }
    let Some(library) = found else {
        return (
            None,
            format!("{software}: this gameDroid has no graphics library ({LIBRARY}) with it."),
        );
    };
    if wish == Wish::Auto && mark(data).exists() {
        return (
            None,
            format!(
                "{software}: the last start with the graphics card did not finish booting. \
                 To try it again, start gameDroid with --gpu on."
            ),
        );
    }
    // A check that failed before is not run again unasked: on a machine
    // without Vulkan it can take the best part of a minute, every start.
    let unusable = data.join("gpu").join("unusable");
    if wish == Wish::Auto {
        if let Ok(why) = std::fs::read_to_string(&unusable) {
            return (
                None,
                format!(
                    "{software}: the graphics card could not be used here when last tried ({}). \
                     To try again, for instance after installing a graphics driver, \
                     start gameDroid with --gpu on.",
                    why.trim()
                ),
            );
        }
    }
    match check(&library) {
        Ok(()) => {
            let _ = std::fs::remove_file(&unusable);
            (
                Some(library),
                "Android will draw with the graphics card.".to_string(),
            )
        }
        Err(why) => {
            if let Some(dir) = unusable.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&unusable, &why);
            (
                None,
                format!("{software}: the graphics card cannot be used here ({why})."),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gd-gpu-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_graphics_card_is_used_only_when_it_is_shown_to_work() {
        let data = folder("decide");
        let lib = Some(PathBuf::from("/somewhere/lib"));
        let works = |_: &Path| Ok(());
        let broken = |_: &Path| Err("no Vulkan".to_string());
        let never_check = |_: &Path| -> Result<(), String> { panic!("must not be checked") };
        // Found and working: used. Found and not working: not, with the reason.
        assert_eq!(decide(Wish::Auto, &data, lib.clone(), &works).0, lib);
        let (used, why) = decide(Wish::Auto, &data, lib.clone(), &broken);
        assert!(used.is_none() && why.contains("no Vulkan"), "{why}");
        // The failure is remembered: the next automatic start does not
        // check again, but asking by name does, and success forgets it.
        let (used, why) = decide(Wish::Auto, &data, lib.clone(), &never_check);
        assert!(
            used.is_none() && why.contains("when last tried (no Vulkan)"),
            "{why}"
        );
        assert_eq!(decide(Wish::On, &data, lib.clone(), &works).0, lib);
        assert_eq!(decide(Wish::Auto, &data, lib.clone(), &works).0, lib);
        // No library, or asked not to: not, and the check is not even run.
        let never = |_: &Path| -> Result<(), String> { panic!("must not be checked") };
        assert!(decide(Wish::Auto, &data, None, &never)
            .1
            .contains("no graphics library"));
        assert!(decide(Wish::Off, &data, lib.clone(), &never)
            .1
            .contains("as asked"));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_start_that_never_booted_is_not_repeated_unasked() {
        let data = folder("mark");
        let lib = Some(PathBuf::from("/somewhere/lib"));
        let works = |_: &Path| Ok(());
        starting(&data);
        // Left by a start that did not boot: automatic choice stays away...
        let (used, why) = decide(Wish::Auto, &data, lib.clone(), &works);
        assert!(used.is_none() && why.contains("--gpu on"), "{why}");
        // ...but asking for it by name tries again...
        assert_eq!(decide(Wish::On, &data, lib.clone(), &works).0, lib);
        // ...and a boot that finishes clears it.
        booted(&data);
        assert_eq!(decide(Wish::Auto, &data, lib.clone(), &works).0, lib);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn the_carried_library_is_unpacked_once() {
        let data = folder("unpack");
        let to = unpack(&data, b"LIBRARY BYTES").unwrap();
        assert_eq!(to, own_copy(&data));
        assert_eq!(std::fs::read(&to).unwrap(), b"LIBRARY BYTES");
        let first = std::fs::metadata(&to).unwrap().modified().unwrap();
        unpack(&data, b"LIBRARY BYTES").unwrap();
        assert_eq!(
            std::fs::metadata(&to).unwrap().modified().unwrap(),
            first,
            "not rewritten"
        );
        // A different one (a newer gameDroid) replaces it.
        unpack(&data, b"A NEWER LIBRARY").unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"A NEWER LIBRARY");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_check_is_judged_by_how_the_other_process_ends() {
        // Stand-ins for "gameDroid checking": programs that succeed and fail.
        let (yes, no) = if cfg!(windows) {
            ("cmd", "cmd")
        } else {
            ("true", "false")
        };
        if cfg!(windows) {
            return; // `cmd` takes its arguments differently; covered on Linux
        }
        assert!(check(Path::new(yes), "--x", Path::new("lib")).is_ok());
        assert!(check(Path::new(no), "--x", Path::new("lib")).is_err());
        assert!(
            check(Path::new("/no/such/program"), "--x", Path::new("lib"))
                .unwrap_err()
                .contains("could not be started")
        );
    }
}
