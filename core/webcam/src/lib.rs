// SPDX-License-Identifier: GPL-3.0-or-later
//! The computer's webcams, as a stream of JPEG pictures.
//!
//! Android's camera wants Motion-JPEG. Most webcams send it themselves and
//! their frames pass through untouched; frames from one that sends raw
//! video are encoded here.
//!
//! On Linux this speaks the kernel's camera interface (V4L2) directly; on
//! Windows it uses Media Foundation through the `nokhwa` crate.

mod convert;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub use convert::{nv12_to_jpeg, yuyv_to_jpeg};

/// A webcam this computer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webcam {
    /// What the system calls it.
    pub name: String,
    /// What to open it by.
    pub id: String,
}

/// The webcams this computer has, the system's first choice first.
pub fn list() -> Vec<Webcam> {
    #[cfg(target_os = "linux")]
    {
        linux::list()
    }
    #[cfg(windows)]
    {
        windows::list()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

/// The webcam to use: the one whose name contains `wanted` (whatever the
/// letters' case), else the first.
pub fn choose(cameras: &[Webcam], wanted: Option<&str>) -> Option<Webcam> {
    let named = wanted.and_then(|part| {
        let part = part.to_lowercase();
        cameras
            .iter()
            .find(|c| c.name.to_lowercase().contains(&part))
    });
    named.or(cameras.first()).cloned()
}

/// An open webcam. Dropping it closes the camera (and puts its light out).
pub struct Capture {
    #[cfg(target_os = "linux")]
    inner: linux::Capture,
    #[cfg(windows)]
    inner: windows::Capture,
}

impl Capture {
    /// Open `camera` for pictures of exactly this size, about `fps` a
    /// second. An error if the camera cannot give that size.
    pub fn open(camera: &Webcam, width: u32, height: u32, fps: u32) -> Result<Capture, String> {
        #[cfg(target_os = "linux")]
        {
            linux::Capture::open(&camera.id, width, height, fps).map(|inner| Capture { inner })
        }
        #[cfg(windows)]
        {
            windows::Capture::open(&camera.id, width, height, fps).map(|inner| Capture { inner })
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (camera, width, height, fps);
            Err("webcams are not supported on this system".into())
        }
    }

    /// The next picture as a JPEG, waiting up to about a second for it.
    /// `None` if none came in that time.
    pub fn frame(&mut self) -> Option<Vec<u8>> {
        #[cfg(any(target_os = "linux", windows))]
        {
            self.inner.frame()
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_is_chosen_by_part_of_its_name_or_the_first_is_taken() {
        let cams = vec![
            Webcam {
                name: "Integrated Webcam".into(),
                id: "0".into(),
            },
            Webcam {
                name: "Logitech BRIO".into(),
                id: "1".into(),
            },
        ];
        assert_eq!(choose(&cams, None).unwrap().id, "0");
        assert_eq!(choose(&cams, Some("brio")).unwrap().id, "1");
        assert_eq!(
            choose(&cams, Some("no such")).unwrap().id,
            "0",
            "falls back to the first"
        );
        assert_eq!(choose(&[], Some("brio")), None);
    }

    #[test]
    fn listing_does_not_fail_on_a_machine_without_cameras() {
        // Whatever this machine has; the call itself must be harmless.
        let _ = list();
    }
}
