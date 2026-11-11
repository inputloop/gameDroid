// SPDX-License-Identifier: GPL-3.0-or-later
//! Webcams on Windows, through Media Foundation (the `nokhwa` crate).

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    Resolution,
};

use crate::Webcam;

pub fn list() -> Vec<Webcam> {
    nokhwa::query(ApiBackend::MediaFoundation)
        .map(|cameras| {
            cameras
                .iter()
                .map(|c| Webcam {
                    name: c.human_name(),
                    id: c.index().to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub struct Capture {
    camera: nokhwa::Camera,
    width: u32,
    height: u32,
}

// SAFETY: the camera is used from one thread at a time (it is moved to the
// thread that reads it and stays there).
unsafe impl Send for Capture {}

impl Capture {
    pub fn open(id: &str, width: u32, height: u32, fps: u32) -> Result<Capture, String> {
        let index = match id.parse::<u32>() {
            Ok(n) => CameraIndex::Index(n),
            Err(_) => CameraIndex::String(id.to_string()),
        };
        // JPEG from the camera if it has it; else the nearest raw format.
        let wanted = CameraFormat::new(Resolution::new(width, height), FrameFormat::MJPEG, fps);
        let request = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(wanted));
        let mut camera = nokhwa::Camera::new(index, request).map_err(|e| e.to_string())?;
        let got = camera.resolution();
        if (got.width(), got.height()) != (width, height) {
            return Err(format!(
                "the camera gives {}x{}, not the {width}x{height} Android asked for",
                got.width(),
                got.height()
            ));
        }
        camera.open_stream().map_err(|e| e.to_string())?;
        Ok(Capture {
            camera,
            width,
            height,
        })
    }

    pub fn frame(&mut self) -> Option<Vec<u8>> {
        let frame = self.camera.frame().ok()?;
        let bytes = frame.buffer();
        match frame.source_frame_format() {
            FrameFormat::MJPEG => (bytes.len() > 4).then(|| bytes.to_vec()),
            FrameFormat::YUYV => crate::yuyv_to_jpeg(bytes, self.width, self.height),
            FrameFormat::NV12 => crate::nv12_to_jpeg(bytes, self.width, self.height),
            _ => None,
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.camera.stop_stream();
    }
}
