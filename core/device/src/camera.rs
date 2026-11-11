// SPDX-License-Identifier: GPL-3.0-or-later
//! What Android's camera sees.
//!
//! [`TestPicture`] is a stand-in for a real webcam: colour bars with a
//! block that moves, so that a frozen picture and a live one can be told
//! apart, sent as JPEG like a webcam's own.

use bvmm::virtio::media::{FrameSource, Mode};

/// The colour bars every video engineer knows, left to right.
const BARS: [[u8; 3]; 8] = [
    [235, 235, 235],
    [235, 235, 16],
    [16, 235, 235],
    [16, 235, 16],
    [235, 16, 235],
    [235, 16, 16],
    [16, 16, 235],
    [16, 16, 16],
];

/// Colour bars with a moving block, at 640x480 or 1280x720, 30 frames a
/// second.
#[derive(Default)]
pub struct TestPicture {
    mode: Option<Mode>,
    frame: u32,
}

impl TestPicture {
    /// Frame number `n` of the picture as RGB bytes.
    pub fn draw(width: usize, height: usize, n: u32) -> Vec<u8> {
        let mut rgb = vec![0u8; width * height * 3];
        for (i, px) in rgb.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            px.copy_from_slice(&BARS[(i % width) * BARS.len() / width]);
        }
        // A white block that crosses the lower third once every 4 seconds.
        let side = height / 6;
        let left = (n as usize % 120) * (width - side) / 120;
        let top = height * 2 / 3;
        for y in top..top + side {
            let row = (y * width + left) * 3;
            rgb[row..row + side * 3].fill(255);
        }
        rgb
    }

    /// One frame as a JPEG picture.
    pub fn jpeg(width: usize, height: usize, n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut out, 85);
        // The colour halved both ways, as webcams send it.
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
        let rgb = Self::draw(width, height, n);
        match encoder.encode(
            &rgb,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgb,
        ) {
            Ok(()) => out,
            Err(_) => Vec::new(),
        }
    }
}

impl FrameSource for TestPicture {
    fn modes(&self) -> Vec<Mode> {
        vec![
            Mode {
                width: 640,
                height: 480,
                fps: 30,
            },
            Mode {
                width: 1280,
                height: 720,
                fps: 30,
            },
        ]
    }

    fn start(&mut self, mode: Mode) {
        eprintln!(
            "gameDroid: Android started the camera ({}x{}, {} frames a second)",
            mode.width, mode.height, mode.fps
        );
        self.mode = Some(mode);
        self.frame = 0;
    }

    fn frame(&mut self) -> Option<Vec<u8>> {
        let mode = self.mode?;
        self.frame += 1;
        let jpeg = Self::jpeg(mode.width as usize, mode.height as usize, self.frame);
        (!jpeg.is_empty()).then_some(jpeg)
    }

    fn stop(&mut self) {
        eprintln!(
            "gameDroid: Android stopped the camera after {} frames",
            self.frame
        );
        self.mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_picture_is_a_jpeg_that_changes() {
        let a = TestPicture::jpeg(640, 480, 1);
        let b = TestPicture::jpeg(640, 480, 30);
        // A JPEG starts FF D8 and ends FF D9, and fits the camera's buffer.
        assert_eq!(
            (&a[..2], &a[a.len() - 2..]),
            (&[0xff, 0xd8][..], &[0xff, 0xd9][..])
        );
        assert!(a.len() < 640 * 480 && a.len() > 2000, "{} bytes", a.len());
        assert_ne!(a, b, "the block has moved");
        assert!(TestPicture::jpeg(1280, 720, 1).len() < 1280 * 720);
        // The bars run white to black; the block sits in the lower third.
        let rgb = TestPicture::draw(640, 480, 0);
        assert_eq!(&rgb[..3], &[235, 235, 235]);
        assert_eq!(&rgb[639 * 3..640 * 3], &[16, 16, 16]);
        assert_eq!(&rgb[(320 * 640) * 3..(320 * 640) * 3 + 3], &[255, 255, 255]);
    }

    #[test]
    fn it_gives_frames_only_while_started() {
        let mut camera = TestPicture::default();
        assert!(camera.frame().is_none());
        camera.start(camera.modes()[0]);
        assert!(camera.frame().is_some());
        camera.stop();
        assert!(camera.frame().is_none());
    }
}
