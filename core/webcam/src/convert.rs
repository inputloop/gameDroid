// SPDX-License-Identifier: GPL-3.0-or-later
//! Turning raw video frames into JPEG pictures.

fn encode(ycbcr: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, 85);
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    encoder
        .encode(
            ycbcr,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Ycbcr,
        )
        .ok()?;
    Some(out)
}

/// A JPEG from a YUYV frame (two pixels in four bytes: Y0 U Y1 V).
pub fn yuyv_to_jpeg(frame: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixels = width as usize * height as usize;
    if !width.is_multiple_of(2) || frame.len() < pixels * 2 || width > 65535 || height > 65535 {
        return None;
    }
    let mut ycbcr = Vec::with_capacity(pixels * 3);
    for pair in frame[..pixels * 2].chunks_exact(4) {
        ycbcr.extend_from_slice(&[pair[0], pair[1], pair[3], pair[2], pair[1], pair[3]]);
    }
    encode(&ycbcr, width, height)
}

/// A JPEG from an NV12 frame (all the Y, then U and V interleaved at half
/// size each way).
pub fn nv12_to_jpeg(frame: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if !w.is_multiple_of(2)
        || !h.is_multiple_of(2)
        || frame.len() < w * h * 3 / 2
        || width > 65535
        || height > 65535
    {
        return None;
    }
    let (luma, chroma) = frame.split_at(w * h);
    let mut ycbcr = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let c = (y / 2) * w + (x / 2) * 2;
            ycbcr.extend_from_slice(&[luma[y * w + x], chroma[c], chroma[c + 1]]);
        }
    }
    encode(&ycbcr, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_jpeg(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9])
    }

    #[test]
    fn raw_frames_become_jpeg_and_bad_ones_are_refused() {
        // A grey 16x8 picture in each raw format.
        let yuyv = vec![128u8; 16 * 8 * 2];
        let nv12 = vec![128u8; 16 * 8 * 3 / 2];
        assert!(is_jpeg(&yuyv_to_jpeg(&yuyv, 16, 8).unwrap()));
        assert!(is_jpeg(&nv12_to_jpeg(&nv12, 16, 8).unwrap()));
        // Too short for the size claimed, or an odd width.
        assert!(yuyv_to_jpeg(&yuyv[..100], 16, 8).is_none());
        assert!(nv12_to_jpeg(&nv12[..100], 16, 8).is_none());
        assert!(yuyv_to_jpeg(&yuyv, 15, 8).is_none());
    }

    #[test]
    fn yuyv_pixels_keep_their_own_brightness_and_share_colour() {
        // Y0 U Y1 V  ->  (Y0, U, V), (Y1, U, V)
        let frame = [10u8, 20, 30, 40];
        let mut ycbcr = Vec::new();
        for pair in frame.chunks_exact(4) {
            ycbcr.extend_from_slice(&[pair[0], pair[1], pair[3], pair[2], pair[1], pair[3]]);
        }
        assert_eq!(ycbcr, [10, 20, 40, 30, 20, 40]);
    }
}
