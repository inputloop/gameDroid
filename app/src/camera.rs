// SPDX-License-Identifier: GPL-3.0-or-later
//! The computer's webcam, behind Android's camera.
//!
//! The webcam is opened when Android starts its camera and closed when
//! Android stops it, so its light is on only while an app is looking. If
//! there is no webcam, or it cannot give the picture size Android asked
//! for, Android sees gameDroid's test picture instead and the reason is
//! said once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bvmm::virtio::media::{FrameSource, Mode};
use gamedroid_device::camera::TestPicture;

pub struct HostCamera {
    /// Part of the name of the webcam to use.
    wanted: Option<String>,
    /// The newest picture from the webcam, until Android takes it.
    latest: Arc<Mutex<Option<Vec<u8>>>>,
    /// Tells the thread reading the webcam to stop, while one runs.
    stop: Option<Arc<AtomicBool>>,
    /// Set by that thread if the webcam could not be used.
    failed: Arc<AtomicBool>,
    stand_in: TestPicture,
    mode: Option<Mode>,
    stand_in_started: bool,
}

impl HostCamera {
    pub fn new(wanted: Option<String>) -> HostCamera {
        HostCamera {
            wanted,
            latest: Arc::default(),
            stop: None,
            failed: Arc::default(),
            stand_in: TestPicture::default(),
            mode: None,
            stand_in_started: false,
        }
    }
}

impl FrameSource for HostCamera {
    fn modes(&self) -> Vec<Mode> {
        // The sizes nearly every webcam has.
        self.stand_in.modes()
    }

    fn start(&mut self, mode: Mode) {
        self.mode = Some(mode);
        self.stand_in_started = false;
        self.failed.store(false, Ordering::SeqCst);
        *self.latest.lock().unwrap() = None;
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(stop.clone());
        let (latest, failed, wanted) = (
            self.latest.clone(),
            self.failed.clone(),
            self.wanted.clone(),
        );
        // Opening a camera can take a second or two: not on Android's time.
        std::thread::spawn(move || {
            let cameras = gamedroid_webcam::list();
            let Some(camera) = gamedroid_webcam::choose(&cameras, wanted.as_deref()) else {
                eprintln!("gameDroid: this computer has no webcam; Android sees a test picture");
                failed.store(true, Ordering::SeqCst);
                return;
            };
            let mut capture =
                match gamedroid_webcam::Capture::open(&camera, mode.width, mode.height, mode.fps) {
                    Ok(capture) => capture,
                    Err(e) => {
                        eprintln!(
                            "gameDroid: {} cannot be used ({e}); Android sees a test picture",
                            camera.name
                        );
                        failed.store(true, Ordering::SeqCst);
                        return;
                    }
                };
            eprintln!(
                "gameDroid: Android is looking through {} ({}x{})",
                camera.name, mode.width, mode.height
            );
            let mut quiet = 0;
            while !stop.load(Ordering::SeqCst) {
                match capture.frame() {
                    Some(picture) => {
                        quiet = 0;
                        *latest.lock().unwrap() = Some(picture);
                    }
                    None => {
                        quiet += 1;
                        // Five seconds without a picture: it is not coming.
                        if quiet >= 5 {
                            eprintln!("gameDroid: {} stopped sending pictures; Android sees a test picture", camera.name);
                            failed.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            }
            eprintln!("gameDroid: {} is closed", camera.name);
        });
    }

    fn frame(&mut self) -> Option<Vec<u8>> {
        if self.failed.load(Ordering::SeqCst) {
            if !self.stand_in_started {
                self.stand_in.start(self.mode?);
                self.stand_in_started = true;
            }
            return self.stand_in.frame();
        }
        self.latest.lock().unwrap().take()
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        if std::mem::take(&mut self.stand_in_started) {
            self.stand_in.stop();
        }
        self.mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_a_webcam_that_cannot_be_used_android_sees_the_test_picture() {
        // A name no camera has: whatever this machine has, none is opened
        // by that name... but `choose` falls back to the first camera, so
        // only the no-camera and failed-camera paths are checked here, by
        // setting the flag the reading thread would set.
        let mut camera = HostCamera::new(None);
        camera.mode = Some(camera.modes()[0]);
        camera.failed.store(true, Ordering::SeqCst);
        let picture = camera.frame().expect("the test picture");
        assert!(picture.starts_with(&[0xff, 0xd8]), "a JPEG");
        camera.stop();
        assert!(camera.mode.is_none() && !camera.stand_in_started);
    }

    #[test]
    fn pictures_from_the_webcam_are_handed_over_once_each() {
        let mut camera = HostCamera::new(None);
        assert!(camera.frame().is_none(), "nothing before one arrives");
        *camera.latest.lock().unwrap() = Some(vec![1, 2, 3]);
        assert_eq!(camera.frame(), Some(vec![1, 2, 3]));
        assert!(
            camera.frame().is_none(),
            "the same picture is not sent twice"
        );
    }
}
