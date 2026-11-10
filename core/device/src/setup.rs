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
/// Room needed in the data folder to set up: the archive, the unpacked
/// images, and what the partitions hold once expanded.
pub const SPACE_NEEDED: u64 = 8 << 30;

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

/// gameDroid's folder for downloaded and generated files.
pub fn data_dir() -> PathBuf {
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = var("GAMEDROID_DATA") {
        return dir;
    }
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

/// Download `url` to `to`, by way of a `.part` file so an interrupted
/// download is never mistaken for a finished one.
pub fn download(url: &str, to: &Path, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let part = to.with_extension("part");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(30))
        .timeout_read(std::time::Duration::from_secs(120))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("downloading the Android image: {e}"))?;
    let total = response
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut body = response.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| io("creating", &part, e))?;
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut reported = 0u64;
    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("downloading the Android image: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| io("writing", &part, e))?;
        done += n as u64;
        if done - reported >= 32 << 20 {
            reported = done;
            progress(Progress::Downloading { done, total });
        }
    }
    if total.is_some_and(|t| t != done) {
        return Err(format!(
            "the download stopped early: {done} of {} bytes",
            total.unwrap_or(0)
        ));
    }
    progress(Progress::Downloading { done, total });
    drop(file);
    std::fs::rename(&part, to).map_err(|e| io("finishing", to, e))
}

/// Unpack the partition images from the archive into `images`, expanding
/// the ones stored in Android's sparse format.
pub fn unpack(
    archive: &Path,
    images: &Path,
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
        let mut member = zip
            .by_name(name)
            .map_err(|e| format!("the archive has no usable {name}: {e}"))?;
        let mut head = [0u8; 4];
        let got = read_up_to(&mut member, &mut head).map_err(|e| format!("reading {name}: {e}"))?;
        let out = images.join(name);
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
                download(&archive_url(BUILD), &downloaded, progress)?;
            }
            downloaded.clone()
        }
    };
    unpack(&archive, &images, progress)?;
    progress(Progress::Preparing);
    prepare::prepare(&images, &prepared, settings)?;
    // The archive is no longer needed; a failure to delete it is harmless.
    let _ = std::fs::remove_file(&downloaded);
    std::fs::write(&ready, format!("build {BUILD}\n")).map_err(|e| io("writing", &ready, e))?;
    Ok(prepared)
}

/// [`ensure`] in gameDroid's own data folder, reporting progress as lines
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
    let data = data_dir();
    let archive = std::env::var_os("GAMEDROID_IMAGE_ZIP").map(PathBuf::from);
    if !data
        .join("images")
        .join(BUILD)
        .join("prepared/READY")
        .exists()
    {
        eprintln!(
            "gameDroid: first run. Setting up Android (build {BUILD}) in {}",
            data.display()
        );
        eprintln!("gameDroid: this downloads about 1.2 GB and needs about 8 GB of disk.");
    }
    let started = std::time::Instant::now();
    let mut worked = false;
    let prepared = ensure(&data, archive.as_deref(), &Settings::default(), &mut |p| {
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
    })?;
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
        let prepared = ensure(&data, Some(&zip), &Settings::default(), &mut |p| {
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
        let same = ensure(&data, Some(&zip), &Settings::default(), &mut |p| {
            again.push(p)
        })
        .unwrap();
        assert_eq!(same, prepared);
        assert!(again.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_bad_archive_is_reported_and_leaves_no_ready_mark() {
        let dir = std::env::temp_dir().join(format!("gd-setup-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("img.zip");
        std::fs::write(&zip, b"this is not a zip file").unwrap();
        let data = dir.join("data");
        let e = ensure(&data, Some(&zip), &Settings::default(), &mut |_| {}).unwrap_err();
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
