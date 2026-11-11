// SPDX-License-Identifier: GPL-3.0-or-later
//! The machine Android runs in.
//!
//! Built from a folder made by `tools/engine/android_prepare.py`: `kernel`,
//! `initrd.img`, `cmdline.txt` and `os.layout`.

use std::path::Path;

use bvmm::virtio::console::Console;
use bvmm::virtio::gpu::{Display, Format, Frame};
use bvmm::virtio::input::{Input, InputSender};
use bvmm::{DiskConfig, VmConfig};

use crate::cuttlefish::{SensorsControl, CONSOLE_PORTS, PHONE_SENSORS, SENSORS_CONTROL_PORT};

/// The guest's vsock address, the one the stock image is configured for.
const GUEST_CID: u64 = 3;
/// The network card's hardware address (locally administered).
const GUEST_MAC: [u8; 6] = [0x02, 0x67, 0x64, 0x00, 0x00, 0x15];

#[derive(Debug, Clone)]
pub struct Options {
    pub cpus: u8,
    pub memory_mib: u64,
    /// Screen size in pixels.
    pub width: u32,
    pub height: u32,
    /// Extra kernel arguments, for diagnosis.
    pub extra_cmdline: String,
    /// An extra read-only disk (`/dev/block/vdb` in Android), used to hand
    /// files such as an APK to the guest.
    pub extra_disk: Option<std::path::PathBuf>,
    /// A network card with the host's internet behind it.
    pub network: bool,
}

/// How many processors to give Android on a host with `host` logical
/// processors, when the engine supports at most `engine_max`: half of the
/// host's, so the host stays responsive, but at least 2 and at most 8.
pub fn auto_cpus(host: usize, engine_max: u8) -> u8 {
    ((host / 2).clamp(2, 8) as u8)
        .min(host.max(1) as u8)
        .min(engine_max)
        .max(1)
}

impl Default for Options {
    fn default() -> Options {
        let host = std::thread::available_parallelism().map_or(1, |n| n.get());
        Options {
            cpus: auto_cpus(host, bvmm::platform::MAX_CPUS),
            memory_mib: 4096,
            width: 720,
            height: 1280,
            extra_cmdline: String::new(),
            extra_disk: None,
            network: true,
        }
    }
}

/// Write a disk layout that presents `file` as a read-only disk, padded with
/// zeros to a whole number of sectors. Returns the layout's path and the
/// file's true length, which the guest needs to cut the padding off again.
pub fn file_as_disk(file: &Path, layout: &Path) -> Result<u64, String> {
    let len = std::fs::metadata(file)
        .map_err(|e| format!("reading {}: {e}", file.display()))?
        .len();
    if len == 0 {
        return Err(format!("{} is empty", file.display()));
    }
    let absolute =
        std::fs::canonicalize(file).map_err(|e| format!("resolving {}: {e}", file.display()))?;
    let padded = len.div_ceil(512) * 512;
    let text = format!("bvmm-disk-layout 1\n{padded} ro {}\n", absolute.display());
    std::fs::write(layout, text).map_err(|e| format!("writing {}: {e}", layout.display()))?;
    Ok(len)
}

/// What the host holds on to while the machine runs.
pub struct Handles {
    /// Read frames from here.
    pub display: Display,
    /// Push touches here.
    pub touch: InputSender,
    /// Push key presses here.
    pub keyboard: InputSender,
}

/// A frame from the stock Android image with its colours put right.
///
/// The image's composer draws into a buffer the guest kernel describes to
/// the display device as blue-green-red, but fills it red-green-blue. A host
/// that believes the description shows red and blue swapped: blue icons come
/// out orange. Seen on 2026-11-10 against a screenshot taken inside the
/// guest; the image's `display_framebuffer_format` setting did not change it.
/// So the description is overridden here, for this image.
pub fn true_colours(mut frame: Frame) -> Frame {
    frame.format = match frame.format {
        Format::Bgra | Format::Bgrx => Format::Rgbx,
        Format::Rgba | Format::Rgbx => Format::Bgrx,
        other => other,
    };
    frame
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// The console ports the image expects, silent except where a service needs
/// an answer to start.
pub fn consoles() -> Vec<Console> {
    (0..CONSOLE_PORTS)
        .map(|port| {
            if port == SENSORS_CONTROL_PORT {
                let mut sensors = SensorsControl::new(PHONE_SENSORS);
                Console::responder(move |bytes| sensors.handle(bytes))
            } else {
                Console::sink()
            }
        })
        .collect()
}

/// Describe the machine for the image set prepared in `dir`.
pub fn machine(dir: &Path, options: &Options) -> Result<(VmConfig, Handles), String> {
    let cmdline = String::from_utf8(read(&dir.join("cmdline.txt"))?)
        .map_err(|_| "cmdline.txt is not text".to_string())?;
    let cmdline = format!("{} {}", cmdline.trim(), options.extra_cmdline.trim());
    let display = Display::new(options.width, options.height);
    let (touchscreen, touch) = Input::touchscreen(options.width, options.height);
    let (keys, keyboard) = Input::keyboard();
    let mut disks = vec![DiskConfig {
        path: dir.join("os.layout"),
        read_only: false,
    }];
    if let Some(path) = &options.extra_disk {
        disks.push(DiskConfig {
            path: path.clone(),
            read_only: true,
        });
    }
    let config = VmConfig {
        memory_mib: options.memory_mib,
        cpus: options.cpus,
        kernel: read(&dir.join("kernel"))?,
        initrd: Some(read(&dir.join("initrd.img"))?),
        cmdline: cmdline.trim().to_string(),
        // The boot configuration names the system disk by its PCI slot, so
        // it must stay the first device.
        disks,
        display: Some(display.clone()),
        inputs: vec![touchscreen, keys],
        consoles: consoles(),
        vsock_cid: Some(GUEST_CID),
        rng: true,
        nets: if options.network {
            vec![bvmm::net::user_nat(GUEST_MAC)]
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    Ok((
        config,
        Handles {
            display,
            touch,
            keyboard,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_corrected_by_swapping_red_and_blue() {
        // One pixel the guest meant as pure blue, written red-first: 00 00 ff.
        let frame = Frame {
            width: 1,
            height: 1,
            format: Format::Bgrx,
            pixels: vec![0x00, 0x00, 0xff, 0xff],
            sequence: 1,
        };
        assert_eq!(frame.to_rgb(), [0xff, 0x00, 0x00], "as labelled: red");
        assert_eq!(
            true_colours(frame).to_rgb(),
            [0x00, 0x00, 0xff],
            "corrected: blue"
        );
    }

    #[test]
    fn the_machine_matches_what_the_image_expects() {
        let dir = std::env::temp_dir().join(format!("gd-machine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kernel"), b"K").unwrap();
        std::fs::write(dir.join("initrd.img"), b"I").unwrap();
        std::fs::write(dir.join("cmdline.txt"), "console=ttyS0 init=/init\n").unwrap();
        let options = Options {
            cpus: 1,
            extra_cmdline: " ignore_loglevel ".into(),
            ..Default::default()
        };
        let (config, handles) = machine(&dir, &options).unwrap();
        assert_eq!(config.cmdline, "console=ttyS0 init=/init ignore_loglevel");
        assert_eq!((config.cpus, config.memory_mib), (1, 4096));
        // Processors follow the host, within what the engine can run.
        assert_eq!(auto_cpus(20, 64), 8);
        assert_eq!(auto_cpus(8, 64), 4);
        assert_eq!(auto_cpus(4, 64), 2);
        assert_eq!(auto_cpus(2, 64), 2);
        assert_eq!(auto_cpus(1, 64), 1);
        assert_eq!(auto_cpus(20, 1), 1);
        assert_eq!(config.disks.len(), 1);
        let apk = dir.join("game.apk");
        std::fs::write(&apk, vec![7u8; 1000]).unwrap();
        let layout = dir.join("apk.layout");
        assert_eq!(file_as_disk(&apk, &layout).unwrap(), 1000);
        let text = std::fs::read_to_string(&layout).unwrap();
        // An absolute path, however this system writes one.
        let line = text.lines().nth(1).unwrap_or_default();
        let path = line
            .strip_prefix("1024 ro ")
            .unwrap_or_else(|| panic!("{text}"));
        assert!(text.starts_with("bvmm-disk-layout 1\n"), "{text}");
        assert!(std::path::Path::new(path).is_absolute(), "{text}");
        let with_disk = Options {
            extra_disk: Some(layout),
            ..Default::default()
        };
        let (two, _) = machine(&dir, &with_disk).unwrap();
        assert_eq!(two.disks.len(), 2);
        assert!(two.disks[1].read_only && !two.disks[0].read_only);
        assert!(config.disks[0].path.ends_with("os.layout"));
        assert_eq!(config.consoles.len(), 20);
        assert_eq!(config.inputs.len(), 2);
        assert_eq!(config.vsock_cid, Some(3));
        assert!(config.rng);
        assert_eq!(handles.display.size(), (720, 1280));
        // Disk, screen, inputs, ports, vsock and the random source must all
        // fit on the one PCI bus.
        let devices = config.disks.len() + 1 + config.inputs.len() + config.consoles.len() + 2;
        assert_eq!(devices, 26);
        assert!(devices <= bvmm::devices::pci::MAX_DEVICES);

        std::fs::remove_file(dir.join("kernel")).unwrap();
        assert!(machine(&dir, &options).err().unwrap().contains("kernel"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
