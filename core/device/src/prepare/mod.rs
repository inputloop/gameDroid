// SPDX-License-Identifier: GPL-3.0-or-later
//! Turning a stock Android image set into what the engine boots.
//!
//! The stock image expects a bootloader that reads the boot partitions,
//! builds the boot configuration and loads the kernel. The engine loads a
//! kernel directly, so this does the bootloader's part ahead of time and
//! writes, into one folder:
//!
//! * `kernel`: the kernel from `boot.img`
//! * `initrd.img`: vendor ramdisk, generic ramdisk, boot configuration
//! * `cmdline.txt`: the kernel command line
//! * `os.layout`: a GPT disk built around the partition images in place
//!
//! (`tools/engine/android_prepare.py` is the earlier Python version of the
//! same thing, kept for the CI jobs that predate this.)

pub mod bootconfig;
pub mod bootimg;
pub mod gpt;
pub mod sparse;

use std::path::Path;

use gpt::{Guid, Partition, MIB};

/// What Android's first-stage init looks for, in the stock device's order:
/// (partition name, image file or None for a blank one, blank size, writable).
pub const PARTITIONS: &[(&str, Option<&str>, u64, bool)] = &[
    ("misc", None, MIB, true),
    ("boot_a", Some("boot.img"), 0, false),
    ("init_boot_a", Some("init_boot.img"), 0, false),
    ("vendor_boot_a", Some("vendor_boot.img"), 0, false),
    ("vbmeta_a", Some("vbmeta.img"), 0, false),
    ("vbmeta_system_a", Some("vbmeta_system.img"), 0, false),
    (
        "vbmeta_vendor_dlkm_a",
        Some("vbmeta_vendor_dlkm.img"),
        0,
        false,
    ),
    (
        "vbmeta_system_dlkm_a",
        Some("vbmeta_system_dlkm.img"),
        0,
        false,
    ),
    ("super", Some("super.img"), 0, true),
    ("userdata", Some("userdata.img"), 0, true),
    ("metadata", None, 64 * MIB, true),
    ("frp", None, MIB, true),
];

#[derive(Debug, Clone)]
pub struct Settings {
    /// Where Android finds its system disk, as a PCI path.
    pub boot_device: String,
    pub memory_mib: u64,
    /// Screen density in dots per inch.
    pub density: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            boot_device: "pci0000:00/0000:00:01.0".into(),
            memory_mib: 4096,
            density: 280,
        }
    }
}

/// What a preparation produced, for reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub kernel_bytes: u64,
    pub initrd_bytes: u64,
    pub disk_bytes: u64,
}

fn io(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| io("reading", path, e))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| io("writing", path, e))
}

/// The kernel command line: the image's own, then what a bootloader adds.
/// The console goes to the 16550 port the engine emulates; Android's kernel
/// has no such ports unless asked (`8250.nr_uarts`).
pub fn kernel_cmdline(vendor_cmdline: &str) -> String {
    let ours = [
        "console=ttyS0",
        "8250.nr_uarts=1",
        "earlycon=uart8250,io,0x3f8",
        "printk.devkmsg=on",
        "audit=1",
        "binder.impl=rust",
        "cma=0",
        "firmware_class.path=/vendor/etc/",
        "loop.max_part=7",
        "init=/init",
        "bootconfig",
    ];
    let mut out: Vec<&str> = Vec::new();
    for word in vendor_cmdline
        .split_whitespace()
        .filter(|w| !w.starts_with("console="))
        .chain(ours)
    {
        if !out.contains(&word) {
            out.push(word);
        }
    }
    out.join(" ")
}

/// Prepare the image set in `images` into `out`.
///
/// The partition images are used where they are: `out/os.layout` refers to
/// them by absolute path, and Android writes to `super.img` and
/// `userdata.img` in place.
pub fn prepare(images: &Path, out: &Path, settings: &Settings) -> Result<Prepared, String> {
    std::fs::create_dir_all(out).map_err(|e| io("creating", out, e))?;
    let boot = bootimg::parse_boot(&read(&images.join("boot.img"))?)
        .map_err(|e| format!("boot.img: {e}"))?;
    let init_boot = bootimg::parse_boot(&read(&images.join("init_boot.img"))?)
        .map_err(|e| format!("init_boot.img: {e}"))?;
    let vendor = bootimg::parse_vendor_boot(&read(&images.join("vendor_boot.img"))?)
        .map_err(|e| format!("vendor_boot.img: {e}"))?;
    if boot.kernel.is_empty() {
        return Err("boot.img has no kernel".into());
    }

    let vbmeta = Guid::from_name("vbmeta_a");
    let ours = bootconfig::runtime(
        &settings.boot_device,
        &vbmeta.to_string(),
        settings.memory_mib,
        settings.density,
    );
    let config = bootconfig::merge(&vendor.bootconfig, &ours)?;

    // The kernel unpacks concatenated ramdisks in order, later files winning.
    let mut initrd = vendor.ramdisk;
    initrd.extend_from_slice(&init_boot.ramdisk);
    let trailer = bootconfig::trailer(&config, initrd.len());
    initrd.extend_from_slice(&trailer);

    write(&out.join("kernel"), &boot.kernel)?;
    write(&out.join("initrd.img"), &initrd)?;
    write(&out.join("bootconfig.txt"), config.as_bytes())?;
    write(
        &out.join("cmdline.txt"),
        format!("{}\n", kernel_cmdline(&vendor.cmdline)).as_bytes(),
    )?;

    let mut partitions = Vec::new();
    let mut lines = vec![
        "bvmm-disk-layout 1".to_string(),
        "# Written by gameDroid.".to_string(),
        format!("{MIB} rw gpt-head.bin"),
    ];
    for (name, source, blank, writable) in PARTITIONS {
        let (path, size) = match source {
            Some(file) => {
                let path = images.join(file);
                let size = std::fs::metadata(&path)
                    .map_err(|e| io("reading", &path, e))?
                    .len();
                let absolute =
                    std::fs::canonicalize(&path).map_err(|e| io("resolving", &path, e))?;
                (absolute, size)
            }
            None => {
                let path = out.join(format!("{name}.img"));
                if !path.exists() {
                    let f = std::fs::File::create(&path).map_err(|e| io("creating", &path, e))?;
                    f.set_len(*blank).map_err(|e| io("sizing", &path, e))?;
                }
                let absolute =
                    std::fs::canonicalize(&path).map_err(|e| io("resolving", &path, e))?;
                (absolute, *blank)
            }
        };
        if size == 0 {
            return Err(format!("{} is empty", path.display()));
        }
        partitions.push(Partition {
            name: name.to_string(),
            bytes: size,
            guid: Guid::from_name(name),
        });
        let mode = if *writable { "rw" } else { "ro" };
        lines.push(format!(
            "{} {mode} {}",
            gpt::align(size, MIB),
            path.display()
        ));
    }
    lines.push(format!("{MIB} rw gpt-tail.bin"));
    let table = gpt::build(&partitions, Guid::from_name("gamedroid-os-disk"))?;
    write(&out.join("gpt-head.bin"), &table.head)?;
    write(&out.join("gpt-tail.bin"), &table.tail)?;
    write(&out.join("os.layout"), (lines.join("\n") + "\n").as_bytes())?;

    Ok(Prepared {
        kernel_bytes: boot.kernel.len() as u64,
        initrd_bytes: initrd.len() as u64,
        disk_bytes: table.disk_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("gd-prepare-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn images(dir: &Path) {
        let put = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).unwrap();
        put(
            "boot.img",
            bootimg::tests::boot_image(&b"KERNEL".repeat(1000), b""),
        );
        put(
            "init_boot.img",
            bootimg::tests::boot_image(b"", b"GENERIC-RAMDISK"),
        );
        put(
            "vendor_boot.img",
            bootimg::tests::vendor_boot_image(
                b"VENDOR-RAMDISK!",
                "console=ttynull stack_depot_disable=on bootconfig",
                "androidboot.boot_devices = \"from-vendor\"\nkernel.x = \"1\"\n",
            ),
        );
        for (name, byte) in [
            ("vbmeta.img", b'V'),
            ("vbmeta_system.img", b'S'),
            ("vbmeta_vendor_dlkm.img", b'W'),
            ("vbmeta_system_dlkm.img", b'X'),
        ] {
            put(name, vec![byte; 65536]);
        }
        let mut sup = b"SUPER".to_vec();
        sup.resize(3 * MIB as usize, 0);
        put("super.img", sup);
        let mut data = b"DATA".to_vec();
        data.resize(MIB as usize + 4096, 0); // not a whole number of MiB
        put("userdata.img", data);
    }

    #[test]
    fn prepares_everything_the_engine_needs() {
        let dir = temp("all");
        let (img, out) = (dir.join("images"), dir.join("out"));
        std::fs::create_dir_all(&img).unwrap();
        images(&img);
        let done = prepare(&img, &out, &Settings::default()).unwrap();

        assert_eq!(
            std::fs::read(out.join("kernel")).unwrap(),
            b"KERNEL".repeat(1000)
        );
        let initrd = std::fs::read(out.join("initrd.img")).unwrap();
        assert!(initrd.starts_with(b"VENDOR-RAMDISK!GENERIC-RAMDISK"));
        assert_eq!(initrd.len() % 4, 0);
        assert_eq!(done.initrd_bytes, initrd.len() as u64);
        let config = bootconfig::read_trailer(&initrd).expect("a valid trailer");
        assert_eq!(
            config,
            std::fs::read_to_string(out.join("bootconfig.txt")).unwrap()
        );
        assert!(
            config.contains("kernel.x = \"1\""),
            "the image's own settings are kept"
        );
        assert!(!config.contains("from-vendor"), "but ours replace theirs");
        assert!(config.contains("pci0000:00/0000:00:01.0"));

        let cmdline = std::fs::read_to_string(out.join("cmdline.txt")).unwrap();
        let words: Vec<&str> = cmdline.split_whitespace().collect();
        assert!(words.contains(&"console=ttyS0") && !words.contains(&"console=ttynull"));
        assert!(words.contains(&"stack_depot_disable=on"));
        assert_eq!(words.iter().filter(|w| **w == "bootconfig").count(), 1);

        let layout = std::fs::read_to_string(out.join("os.layout")).unwrap();
        let pieces: Vec<(u64, &str, &str)> = layout
            .lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .map(|l| {
                let mut p = l.splitn(3, ' ');
                (
                    p.next().unwrap().parse().unwrap(),
                    p.next().unwrap(),
                    p.next().unwrap(),
                )
            })
            .collect();
        assert_eq!(pieces.len(), PARTITIONS.len() + 2);
        assert_eq!(pieces.iter().map(|p| p.0).sum::<u64>(), done.disk_bytes);
        assert!(pieces.iter().all(|p| p.0 % MIB == 0));
        let find = |name: &str| pieces.iter().find(|p| p.2.ends_with(name)).unwrap();
        assert_eq!(find("super.img").1, "rw");
        assert_eq!(find("boot.img").1, "ro");
        assert_eq!(find("userdata.img").0, 2 * MIB, "rounded up to a whole MiB");
        assert!(Path::new(find("super.img").2).is_absolute());
        assert_eq!(
            std::fs::metadata(out.join("metadata.img")).unwrap().len(),
            64 * MIB
        );

        // The engine opens the layout, and the partition table in it is sound.
        let mut disk = bvmm::disk::Disk::open(&out.join("os.layout"), false).unwrap();
        assert_eq!(disk.len(), done.disk_bytes);
        let mut whole = vec![0u8; disk.len() as usize];
        disk.read_at(0, &mut whole).unwrap();
        let found = gpt::tests::check(&whole);
        let names: Vec<&str> = found.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(names, PARTITIONS.iter().map(|p| p.0).collect::<Vec<_>>());
        for (name, want) in [
            ("boot_a", &b"ANDROID!"[..]),
            ("super", b"SUPER"),
            ("userdata", b"DATA"),
            ("vbmeta_a", b"VVVV"),
            ("vendor_boot_a", b"VNDRBOOT"),
        ] {
            let at = found.iter().find(|p| p.0 == name).unwrap().1 as usize;
            assert_eq!(&whole[at..at + want.len()], want, "{name}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_broken_images_are_reported_by_name() {
        let dir = temp("bad");
        let (img, out) = (dir.join("images"), dir.join("out"));
        std::fs::create_dir_all(&img).unwrap();
        let e = prepare(&img, &out, &Settings::default()).unwrap_err();
        assert!(e.contains("boot.img"), "{e}");
        images(&img);
        std::fs::write(img.join("vendor_boot.img"), b"nonsense").unwrap();
        let e = prepare(&img, &out, &Settings::default()).unwrap_err();
        assert!(e.contains("vendor_boot.img"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_command_line_keeps_theirs_and_adds_ours_once() {
        assert_eq!(
            kernel_cmdline("console=ttynull foo=1 bootconfig"),
            "foo=1 bootconfig console=ttyS0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8 \
             printk.devkmsg=on audit=1 binder.impl=rust cma=0 firmware_class.path=/vendor/etc/ \
             loop.max_part=7 init=/init"
        );
    }
}
