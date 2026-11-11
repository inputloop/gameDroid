// SPDX-License-Identifier: GPL-3.0-or-later
//! First-run setup: get the stock Android image and make it bootable.
//!
//! The image is Google's public build of the Android virtual device, taken
//! from the public build website by its ordinary download address. It is
//! downloaded to the user's data folder, not shipped with gameDroid.
//!
//! Layout under the data folder:
//!
//! ```text
//! images/<build>/            the unpacked partition images
//! images/<build>/prepared/   kernel, initrd.img, cmdline.txt, os.layout
//! images/<build>/prepared/READY   written last; its presence means done
//! ```

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::prepare::{self, sparse, Settings};

/// The Android build gameDroid is tested with (Android 17).
pub const BUILD: &str = "16644040";
const TARGET: &str = "aosp_cf_x86_64_only_phone-userdebug";
const PRODUCT: &str = "aosp_cf_x86_64_only_phone";
/// The archive's size and SHA-256, checked after the download.
pub const ARCHIVE_BYTES: u64 = 1_163_636_385;
pub const ARCHIVE_SHA256: Option<&str> =
    Some("eccdb248e171af6cb98711a6f2fbd5e5017e9c3a6e82337580181bbd4bb2c41b");
/// Free room needed to set up, before any game is installed: the archive
/// (1.2 GB, deleted afterwards) and the system partitions (about 2 GB).
pub const SPACE_NEEDED: u64 = 5 << 30;
/// Room for games and their data when nothing else is chosen, in GiB.
pub const DEFAULT_STORAGE_GIB: u64 = 32;
/// The least room for games and data that is worth having, in GiB.
pub const MIN_STORAGE_GIB: u64 = 4;

/// Members of the archive that gameDroid uses.
const MEMBERS: &[&str] = &[
    "boot.img",
    "init_boot.img",
    "vendor_boot.img",
    "vbmeta.img",
    "vbmeta_system.img",
    "vbmeta_vendor_dlkm.img",
    "vbmeta_system_dlkm.img",
    "super.img",
    "userdata.img",
];

/// Where the image archive for `build` is downloaded from.
pub fn archive_url(build: &str) -> String {
    format!(
        "https://ci.android.com/builds/submitted/{build}/{TARGET}/latest/raw/{PRODUCT}-img-{build}.zip"
    )
}

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// gameDroid's own small folder, where its settings are kept, and where
/// Android goes too unless another place is chosen.
pub fn home_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(dir) = var("LOCALAPPDATA") {
            return dir.join("gameDroid");
        }
    }
    if let Some(dir) = var("XDG_DATA_HOME") {
        return dir.join("gamedroid");
    }
    match var("HOME") {
        Some(home) => home.join(".local/share/gamedroid"),
        None => PathBuf::from("gamedroid-data"),
    }
}

/// What setup is doing, for whoever shows progress.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    Downloading { done: u64, total: Option<u64> },
    Unpacking { file: String },
    Preparing,
}

fn io(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Download `url` to `to`, by way of a `.part` file so an interrupted
/// download is never mistaken for a finished one. A `.part` file left by an
/// earlier attempt is continued, not started again. Returns the SHA-256 of
/// the whole file.
pub fn download(
    url: &str,
    to: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<String, String> {
    use sha2::Digest;
    let part = to.with_extension("part");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(30))
        .timeout_read(std::time::Duration::from_secs(120))
        .build();
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut request = agent.get(url);
    if have > 0 {
        request = request.set("Range", &format!("bytes={have}-"));
    }
    let response = match request.call() {
        Ok(r) => r,
        // "Range not satisfiable": what we have is not the start of the file.
        Err(ureq::Error::Status(416, _)) if have > 0 => {
            std::fs::remove_file(&part).map_err(|e| io("removing", &part, e))?;
            return download(url, to, progress);
        }
        Err(e) => return Err(format!("downloading the Android image: {e}")),
    };
    let resumed = have > 0 && response.status() == 206;
    let length = response
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut hash = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut file = if resumed {
        // Hash what is already here, then carry on after it.
        let mut old = std::fs::File::open(&part).map_err(|e| io("opening", &part, e))?;
        loop {
            let n = old.read(&mut buf).map_err(|e| io("reading", &part, e))?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            done += n as u64;
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .map_err(|e| io("opening", &part, e))?
    } else {
        std::fs::File::create(&part).map_err(|e| io("creating", &part, e))?
    };
    let total = length.map(|l| l + done);
    let again = "Start gameDroid again and it carries on from there";
    let mut body = response.into_reader();
    let mut reported = done;
    loop {
        let n = body.read(&mut buf).map_err(|e| {
            format!(
                "the download was interrupted after {} MB ({e}). {again}",
                done >> 20
            )
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| io("writing", &part, e))?;
        hash.update(&buf[..n]);
        done += n as u64;
        if done - reported >= 32 << 20 {
            reported = done;
            progress(Progress::Downloading { done, total });
        }
    }
    if let Some(total) = total.filter(|t| *t != done) {
        return Err(format!(
            "the download stopped early, at {done} of {total} bytes. {again}"
        ));
    }
    progress(Progress::Downloading { done, total });
    drop(file);
    std::fs::rename(&part, to).map_err(|e| io("finishing", to, e))?;
    Ok(hex(&hash.finalize()))
}

/// Check a downloaded archive against what is expected of it. A wrong
/// archive is deleted, so the next start downloads it afresh.
fn check_archive(path: &Path, sha256: &str) -> Result<(), String> {
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let wrong = if bytes != ARCHIVE_BYTES {
        Some(format!("it is {bytes} bytes, not {ARCHIVE_BYTES}"))
    } else {
        match ARCHIVE_SHA256 {
            Some(want) if want != sha256 => Some(format!("its SHA-256 is {sha256}, not {want}")),
            _ => None,
        }
    };
    match wrong {
        None => Ok(()),
        Some(why) => {
            let _ = std::fs::remove_file(path);
            Err(format!(
                "the downloaded Android image is not the expected one: {why}. \
                 It has been deleted; start gameDroid again to download it afresh"
            ))
        }
    }
}

/// Unpack the partition images from the archive into `images`, expanding
/// the ones stored in Android's sparse format.
///
/// `storage` is the size in bytes of the partition for games and their
/// data. Android formats a blank partition on its first start, so a blank
/// file of that size is made instead of unpacking the archive's own
/// (8 GiB, empty) one. 0 keeps the archive's.
pub fn unpack(
    archive: &Path,
    images: &Path,
    storage: u64,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    std::fs::create_dir_all(images).map_err(|e| io("creating", images, e))?;
    let file = std::fs::File::open(archive).map_err(|e| io("opening", archive, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("{} is not a usable archive: {e}", archive.display()))?;
    for name in MEMBERS {
        progress(Progress::Unpacking {
            file: name.to_string(),
        });
        let out = images.join(name);
        if *name == "userdata.img" && storage > 0 {
            let dst = std::fs::File::create(&out).map_err(|e| io("creating", &out, e))?;
            sparse::allow_holes(&dst);
            dst.set_len(storage).map_err(|e| io("sizing", &out, e))?;
            continue;
        }
        let mut member = zip
            .by_name(name)
            .map_err(|e| format!("the archive has no usable {name}: {e}"))?;
        let mut head = [0u8; 4];
        let got = read_up_to(&mut member, &mut head).map_err(|e| format!("reading {name}: {e}"))?;
        if sparse::is_sparse(&head[..got]) {
            // Expand straight from the archive: no intermediate copy.
            let mut whole = std::io::Read::chain(&head[..got], member);
            let mut dst = std::fs::File::create(&out).map_err(|e| io("creating", &out, e))?;
            sparse::allow_holes(&dst);
            let total = sparse::expand(&mut whole, &mut dst).map_err(|e| format!("{name}: {e}"))?;
            dst.set_len(total).map_err(|e| io("sizing", &out, e))?;
        } else {
            let mut dst = std::fs::File::create(&out).map_err(|e| io("creating", &out, e))?;
            dst.write_all(&head[..got])
                .map_err(|e| io("writing", &out, e))?;
            std::io::copy(&mut member, &mut dst).map_err(|e| io("writing", &out, e))?;
        }
    }
    Ok(())
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Make sure a prepared image set exists under `data`, downloading and
/// preparing it if it does not. Returns the prepared folder.
///
/// `archive` may name an already-downloaded archive to use instead of
/// downloading one.
pub fn ensure(
    data: &Path,
    archive: Option<&Path>,
    storage: u64,
    settings: &Settings,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, String> {
    let images = data.join("images").join(BUILD);
    let prepared = images.join("prepared");
    let ready = prepared.join("READY");
    if ready.exists() {
        return Ok(prepared);
    }
    std::fs::create_dir_all(&images).map_err(|e| io("creating", &images, e))?;
    let downloaded = images.join("image.zip");
    let archive = match archive {
        Some(path) => path.to_path_buf(),
        None => {
            if !downloaded.exists() {
                let sha256 = download(&archive_url(BUILD), &downloaded, progress)?;
                eprintln!("gameDroid: the archive's SHA-256 is {sha256}");
                check_archive(&downloaded, &sha256)?;
            }
            downloaded.clone()
        }
    };
    unpack(&archive, &images, storage, progress)?;
    progress(Progress::Preparing);
    prepare::prepare(&images, &prepared, settings)?;
    // The archive is no longer needed; a failure to delete it is harmless.
    let _ = std::fs::remove_file(&downloaded);
    std::fs::write(&ready, format!("build {BUILD}\n")).map_err(|e| io("writing", &ready, e))?;
    Ok(prepared)
}

/// Free bytes on the filesystem that holds `path` (or would, if `path`
/// does not exist yet). `None` if the system will not say.
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
        // SAFETY: a valid C string and a zeroed struct for the result.
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            (libc::statvfs(c.as_ptr(), &mut st) == 0)
                .then(|| st.f_bavail as u64 * st.f_frsize as u64)
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                dir: *const u16,
                free_to_caller: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let wide: Vec<u16> = existing.as_os_str().encode_wide().chain([0]).collect();
        let mut free = 0u64;
        // SAFETY: a NUL-terminated path and one valid out-pointer.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
}

/// Places Android could be kept: gameDroid's own folder first, then a
/// `gameDroid` folder on each other fixed drive (Windows).
pub fn places() -> Vec<PathBuf> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut out = vec![home_dir()];
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetDriveTypeW(root: *const u16) -> u32;
        }
        const DRIVE_FIXED: u32 = 3;
        let home = out[0].to_string_lossy().to_uppercase();
        // SAFETY: no arguments; returns a bit per drive letter.
        let drives = unsafe { GetLogicalDrives() };
        for i in 0..26u8 {
            if drives & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i) as char;
            let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated drive root.
            if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_FIXED
                && !home.starts_with(&format!("{letter}:"))
            {
                out.push(PathBuf::from(format!("{letter}:\\gameDroid")));
            }
        }
    }
    out
}

/// Where Android is kept and how much room games and data may take.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub data: PathBuf,
    pub storage_gib: u64,
}

impl Plan {
    /// The settings file's text for this plan.
    pub fn to_text(&self) -> String {
        format!(
            "# gameDroid's settings. Delete this file to be asked again.\n\
             data = {}\nstorage_gib = {}\n",
            self.data.display(),
            self.storage_gib
        )
    }

    /// Read a plan back from the settings file's text.
    pub fn from_text(text: &str) -> Option<Plan> {
        let get = |key: &str| {
            text.lines()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .map(|(_, v)| v.trim().to_string())
        };
        Some(Plan {
            data: PathBuf::from(get("data")?),
            storage_gib: get("storage_gib")?.parse().ok()?,
        })
    }
}

/// The room to offer for games and data on a disk with `free` bytes free:
/// the usual amount, or less if the disk cannot hold that beside Android.
pub fn suggested_storage_gib(free: Option<u64>) -> u64 {
    match free {
        Some(free) => {
            let room = free.saturating_sub(SPACE_NEEDED) >> 30;
            DEFAULT_STORAGE_GIB.min(room).max(MIN_STORAGE_GIB)
        }
        None => DEFAULT_STORAGE_GIB,
    }
}

fn gb(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) => format!("{} GB free", b >> 30),
        None => "free space unknown".into(),
    }
}

/// Ask the person where Android should go and how much room it may take.
/// `answers` yields their typed lines; a blank line takes the default.
pub fn ask(
    places: &[PathBuf],
    free: &dyn Fn(&Path) -> Option<u64>,
    answers: &mut dyn FnMut() -> Option<String>,
    say: &mut dyn FnMut(String),
) -> Plan {
    say("gameDroid keeps Android (about 3 GB), your games and their data in one folder.".into());
    say("Where should that folder be?".into());
    for (i, place) in places.iter().enumerate() {
        say(format!(
            "  {}. {}   ({})",
            i + 1,
            place.display(),
            gb(free(place))
        ));
    }
    let data = loop {
        say("Type a number, or a full folder path, then Enter [1]:".into());
        let line = answers().unwrap_or_default();
        let line = line.trim();
        if line.is_empty() {
            break places[0].clone();
        }
        if let Ok(n) = line.parse::<usize>() {
            if (1..=places.len()).contains(&n) {
                break places[n - 1].clone();
            }
            say(format!("There is no choice {n}."));
            continue;
        }
        let path = PathBuf::from(line);
        if path.is_absolute() {
            break path;
        }
        say("That is not a full path (on Windows it starts with a drive, like D:\\Games).".into());
    };
    let available = free(&data);
    let suggested = suggested_storage_gib(available);
    say(format!(
        "How much room may games and their data use, in GB? ({} there.)",
        gb(available)
    ));
    say("It is a limit, not a reservation: disk is used only as Android fills it.".into());
    let storage_gib = loop {
        say(format!("Type a number of GB, then Enter [{suggested}]:"));
        let line = answers().unwrap_or_default();
        let line = line.trim().trim_end_matches(['G', 'g', 'B', 'b']).trim();
        if line.is_empty() {
            break suggested;
        }
        match line.parse::<u64>() {
            Ok(n) if n < MIN_STORAGE_GIB => say(format!(
                "Android needs at least {MIN_STORAGE_GIB} GB for games and data."
            )),
            Ok(n) if n > 2048 => say("That is more than 2048 GB; please choose less.".into()),
            Ok(n) => {
                if available.is_some_and(|f| (n << 30) + SPACE_NEEDED > f) {
                    say(
                        "Note: that is more than is free there now. Android will run, \
                         but will fail to save once the disk is full."
                            .into(),
                    );
                }
                break n;
            }
            Err(_) => say("Please type a whole number, such as 64.".into()),
        }
    };
    Plan { data, storage_gib }
}

/// The plan to use: from the environment, else the settings file, else by
/// asking (when someone is there to answer), else the defaults.
fn plan() -> Result<Plan, String> {
    use std::io::IsTerminal;
    let settings = home_dir().join("settings.txt");
    let saved = std::fs::read_to_string(&settings)
        .ok()
        .and_then(|t| Plan::from_text(&t));
    let env_storage = std::env::var("GAMEDROID_STORAGE_GB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    let mut plan = match saved {
        Some(plan) => plan,
        None => {
            let unattended = var("GAMEDROID_DATA").is_some() || !std::io::stdin().is_terminal();
            let plan = if unattended {
                let data = home_dir();
                let storage_gib = suggested_storage_gib(free_space(&data));
                Plan { data, storage_gib }
            } else {
                ask(
                    &places(),
                    &free_space,
                    &mut || {
                        let mut line = String::new();
                        std::io::stdin().read_line(&mut line).ok().map(|_| line)
                    },
                    &mut |text| eprintln!("{text}"),
                )
            };
            if var("GAMEDROID_DATA").is_none() {
                let home = home_dir();
                std::fs::create_dir_all(&home).map_err(|e| io("creating", &home, e))?;
                std::fs::write(&settings, plan.to_text())
                    .map_err(|e| io("writing", &settings, e))?;
            }
            plan
        }
    };
    if let Some(dir) = var("GAMEDROID_DATA") {
        plan.data = dir;
    }
    if let Some(gib) = env_storage {
        plan.storage_gib = gib.max(MIN_STORAGE_GIB);
    }
    Ok(plan)
}

/// [`ensure`] in the place the person chose, reporting progress as lines
/// on standard error. The archive can be supplied with `GAMEDROID_IMAGE_ZIP`.
pub fn ensure_default() -> Result<PathBuf, String> {
    // Say what is missing before spending minutes on a download.
    let host = bvmm::probe();
    if !host.usable {
        return Err(format!(
            "this machine cannot run Android yet. {}",
            host.detail
        ));
    }
    let plan = plan()?;
    let data = plan.data.clone();
    let archive = std::env::var_os("GAMEDROID_IMAGE_ZIP").map(PathBuf::from);
    let first = !data
        .join("images")
        .join(BUILD)
        .join("prepared/READY")
        .exists();
    if first {
        eprintln!(
            "gameDroid: first run. Setting up Android (build {BUILD}) in {}",
            data.display()
        );
        eprintln!(
            "gameDroid: this downloads about 1.2 GB; games and data may use up to {} GB.",
            plan.storage_gib
        );
        if let Some(free) = free_space(&data).filter(|f| *f < SPACE_NEEDED) {
            return Err(format!(
                "not enough free disk in {}: {:.1} GB free, and setting up needs {} GB. \
                 Free some space, or delete {} to choose another place",
                data.display(),
                free as f64 / (1u64 << 30) as f64,
                SPACE_NEEDED >> 30,
                home_dir().join("settings.txt").display()
            ));
        }
    }
    let started = std::time::Instant::now();
    let mut worked = false;
    let prepared = ensure(
        &data,
        archive.as_deref(),
        plan.storage_gib << 30,
        &Settings::default(),
        &mut |p| {
            worked = true;
            match p {
                Progress::Downloading { done, total } => match total {
                    Some(total) => eprintln!(
                        "gameDroid: downloading, {} of {} MB",
                        done >> 20,
                        total >> 20
                    ),
                    None => eprintln!("gameDroid: downloading, {} MB", done >> 20),
                },
                Progress::Unpacking { file } => eprintln!("gameDroid: unpacking {file}"),
                Progress::Preparing => eprintln!("gameDroid: preparing the boot files"),
            }
        },
    )?;
    if worked {
        eprintln!(
            "gameDroid: setup finished in {} s",
            started.elapsed().as_secs()
        );
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare::bootimg::tests::{boot_image, vendor_boot_image};
    use crate::prepare::sparse::tests::build as sparse_image;

    /// An archive shaped like the real one, in miniature.
    fn archive(path: &Path) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, bytes: Vec<u8>| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&bytes).unwrap();
        };
        put("android-info.txt", b"not used".to_vec());
        put("boot.img", boot_image(&b"KERNEL".repeat(100), b""));
        put("init_boot.img", boot_image(b"", b"GENERIC"));
        put(
            "vendor_boot.img",
            vendor_boot_image(b"VENDOR", "bootconfig", "a = 1\n"),
        );
        for name in [
            "vbmeta.img",
            "vbmeta_system.img",
            "vbmeta_vendor_dlkm.img",
            "vbmeta_system_dlkm.img",
        ] {
            put(name, vec![b'V'; 4096]);
        }
        // super: one block of data, then 255 blocks of nothing (1 MiB).
        let mut block = b"SUPER".to_vec();
        block.resize(4096, 0);
        put(
            "super.img",
            sparse_image(&[(0xcac1, 1, block), (0xcac3, 255, vec![])], 256),
        );
        put("userdata.img", sparse_image(&[(0xcac3, 512, vec![])], 512));
        zip.finish().unwrap();
    }

    #[test]
    fn sets_up_from_an_archive_once() {
        let dir = std::env::temp_dir().join(format!("gd-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("img.zip");
        archive(&zip);
        let data = dir.join("data");
        let mut seen = Vec::new();
        let prepared = ensure(&data, Some(&zip), 0, &Settings::default(), &mut |p| {
            seen.push(p)
        })
        .unwrap();

        assert!(prepared.join("READY").exists());
        for f in ["kernel", "initrd.img", "cmdline.txt", "os.layout"] {
            assert!(prepared.join(f).exists(), "{f}");
        }
        let images = data.join("images").join(BUILD);
        let sup = std::fs::read(images.join("super.img")).unwrap();
        assert_eq!(sup.len(), 256 * 4096, "expanded to its full size");
        assert!(sup.starts_with(b"SUPER") && sup[4096..].iter().all(|b| *b == 0));
        assert_eq!(
            std::fs::metadata(images.join("userdata.img"))
                .unwrap()
                .len(),
            512 * 4096
        );
        assert!(std::fs::read(images.join("boot.img"))
            .unwrap()
            .starts_with(b"ANDROID!"));
        assert_eq!(seen.last(), Some(&Progress::Preparing));
        assert!(seen.contains(&Progress::Unpacking {
            file: "super.img".into()
        }));
        // The engine can open the disk that was laid out.
        assert!(bvmm::disk::Disk::open(&prepared.join("os.layout"), false).is_ok());

        // A second call does nothing: no archive is needed any more.
        std::fs::remove_file(&zip).unwrap();
        let mut again = Vec::new();
        let same = ensure(&data, Some(&zip), 0, &Settings::default(), &mut |p| {
            again.push(p)
        })
        .unwrap();
        assert_eq!(same, prepared);
        assert!(again.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_chosen_storage_size_makes_a_blank_partition_of_that_size() {
        let dir = std::env::temp_dir().join(format!("gd-setup-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("img.zip");
        archive(&zip);
        let data = dir.join("data");
        let prepared = ensure(
            &data,
            Some(&zip),
            3 << 20,
            &Settings::default(),
            &mut |_| {},
        )
        .unwrap();
        let userdata = std::fs::read(data.join("images").join(BUILD).join("userdata.img")).unwrap();
        assert_eq!(userdata.len(), 3 << 20);
        assert!(
            userdata.iter().all(|b| *b == 0),
            "blank, for Android to format"
        );
        let layout = std::fs::read_to_string(prepared.join("os.layout")).unwrap();
        assert!(
            layout
                .lines()
                .any(|l| l.starts_with(&format!("{} rw ", 3 << 20)) && l.ends_with("userdata.img")),
            "{layout}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asking_takes_defaults_numbers_and_paths_and_insists_on_sense() {
        let places = [PathBuf::from("/home/me/gd"), PathBuf::from("/mnt/big/gd")];
        let free = |p: &Path| {
            Some(if p.starts_with("/mnt/big") {
                500u64 << 30
            } else {
                20 << 30
            })
        };
        let run = |typed: &[&str]| {
            let mut lines = typed.iter().map(|s| s.to_string());
            let mut said = Vec::new();
            let plan = ask(&places, &free, &mut || lines.next(), &mut |t| said.push(t));
            (plan, said.join("\n"))
        };
        // Enter twice: the first place, and as much as fits beside Android.
        let (plan, said) = run(&["", ""]);
        assert_eq!(
            plan,
            Plan {
                data: places[0].clone(),
                storage_gib: 15
            }
        );
        assert!(said.contains("2. /mnt/big/gd   (500 GB free)"), "{said}");
        // The second place, with room for the usual amount; then a larger one.
        assert_eq!(
            run(&["2", ""]).0,
            Plan {
                data: places[1].clone(),
                storage_gib: 32
            }
        );
        assert_eq!(run(&["2", "128 GB"]).0.storage_gib, 128);
        // Nonsense is refused and asked again.
        // A full path, as this system writes one.
        let typed = if cfg!(windows) { "E:\\x" } else { "/srv/x" };
        let (plan, said) = run(&["7", "games", typed, "2", "lots", "64"]);
        assert_eq!(
            plan,
            Plan {
                data: PathBuf::from(typed),
                storage_gib: 64
            }
        );
        assert!(said.contains("no choice 7") && said.contains("not a full path"));
        assert!(said.contains("at least 4 GB") && said.contains("whole number"));
        // More than is free is allowed, with a warning.
        assert!(run(&["1", "100"]).1.contains("more than is free"));
        // Nobody answering takes the defaults rather than waiting for ever.
        assert_eq!(run(&[]).0.data, places[0]);
    }

    /// A one-request-at-a-time web server for `body` that honours `Range`.
    /// Returns its address and a list of the Range headers it was sent.
    fn serve(body: Vec<u8>, requests: usize) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image.zip", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut range = String::new();
                for line in BufReader::new(stream.try_clone().unwrap()).lines() {
                    let line = line.unwrap();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.to_string();
                    }
                }
                let from: usize = range.trim_end_matches('-').parse().unwrap_or(0);
                tx.send(range).unwrap();
                let head = if from >= body.len() && from > 0 {
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n".to_string()
                } else if from > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\r\n",
                        body.len() - from
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
                };
                stream.write_all(head.as_bytes()).unwrap();
                if from < body.len() {
                    stream.write_all(&body[from..]).unwrap();
                }
            }
        });
        (url, rx)
    }

    #[test]
    fn an_interrupted_download_is_continued_and_hashed_whole() {
        use sha2::Digest;
        let dir = std::env::temp_dir().join(format!("gd-setup-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let body: Vec<u8> = (0..300_000u32).map(|i| (i * 7 + i / 251) as u8).collect();
        let want = hex(&sha2::Sha256::digest(&body));
        let to = dir.join("image.zip");

        // From nothing.
        let (url, ranges) = serve(body.clone(), 1);
        assert_eq!(download(&url, &to, &mut |_| {}).unwrap(), want);
        assert_eq!(std::fs::read(&to).unwrap(), body);
        assert_eq!(ranges.recv().unwrap(), "", "no Range on a fresh download");

        // From a part left by an earlier attempt: only the rest is asked for.
        std::fs::remove_file(&to).unwrap();
        std::fs::write(dir.join("image.part"), &body[..100_000]).unwrap();
        let (url, ranges) = serve(body.clone(), 1);
        assert_eq!(download(&url, &to, &mut |_| {}).unwrap(), want);
        assert_eq!(std::fs::read(&to).unwrap(), body);
        assert_eq!(ranges.recv().unwrap(), "100000-");
        assert!(!dir.join("image.part").exists());

        // A part that is not the start of this file is thrown away.
        std::fs::remove_file(&to).unwrap();
        std::fs::write(dir.join("image.part"), vec![0u8; 400_000]).unwrap();
        let (url, ranges) = serve(body.clone(), 2);
        assert_eq!(download(&url, &to, &mut |_| {}).unwrap(), want);
        assert_eq!(std::fs::read(&to).unwrap(), body);
        assert_eq!(
            (ranges.recv().unwrap(), ranges.recv().unwrap()),
            ("400000-".into(), "".into())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_plan_survives_being_saved() {
        let plan = Plan {
            data: PathBuf::from("/mnt/big/gd"),
            storage_gib: 64,
        };
        assert_eq!(Plan::from_text(&plan.to_text()), Some(plan));
        assert_eq!(Plan::from_text("data = /x\n"), None);
        assert!(free_space(&std::env::temp_dir().join("not/there/yet")).is_some());
    }

    #[test]
    fn a_bad_archive_is_reported_and_leaves_no_ready_mark() {
        let dir = std::env::temp_dir().join(format!("gd-setup-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("img.zip");
        std::fs::write(&zip, b"this is not a zip file").unwrap();
        let data = dir.join("data");
        let e = ensure(&data, Some(&zip), 0, &Settings::default(), &mut |_| {}).unwrap_err();
        assert!(e.contains("not a usable archive"), "{e}");
        assert!(!data
            .join("images")
            .join(BUILD)
            .join("prepared/READY")
            .exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_download_address_names_the_pinned_build() {
        assert_eq!(
            archive_url("123"),
            "https://ci.android.com/builds/submitted/123/aosp_cf_x86_64_only_phone-userdebug/latest/raw/aosp_cf_x86_64_only_phone-img-123.zip"
        );
    }
}
