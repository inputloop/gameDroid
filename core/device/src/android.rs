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

#[derive(Debug, Clone)]
pub struct Options {
    pub cpus: u8,
    pub memory_mib: u64,
    /// Screen size in pixels.
    pub width: u32,
    pub height: u32,
    /// Extra kernel arguments, for diagnosis.
    pub extra_cmdline: String,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            cpus: 2,
            memory_mib: 4096,
            width: 720,
            height: 1280,
            extra_cmdline: String::new(),
        }
    }
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
    let config = VmConfig {
        memory_mib: options.memory_mib,
        cpus: options.cpus,
        kernel: read(&dir.join("kernel"))?,
        initrd: Some(read(&dir.join("initrd.img"))?),
        cmdline: cmdline.trim().to_string(),
        // The boot configuration names the system disk by its PCI slot, so
        // it must stay the first device.
        disks: vec![DiskConfig {
            path: dir.join("os.layout"),
            read_only: false,
        }],
        display: Some(display.clone()),
        inputs: vec![touchscreen, keys],
        consoles: consoles(),
        vsock_cid: Some(GUEST_CID),
        rng: true,
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
            extra_cmdline: " ignore_loglevel ".into(),
            ..Default::default()
        };
        let (config, handles) = machine(&dir, &options).unwrap();
        assert_eq!(config.cmdline, "console=ttyS0 init=/init ignore_loglevel");
        assert_eq!((config.cpus, config.memory_mib), (2, 4096));
        assert_eq!(config.disks.len(), 1);
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
