// SPDX-License-Identifier: GPL-3.0-or-later
//! Fitting the guest's screen into the window.

/// Where the guest's picture sits inside a window: scaled to fit, centred,
/// with bars on the sides or top and bottom when the shapes differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Fit {
    /// Fit a `guest` sized picture into a `window`.
    pub fn new(guest: (u32, u32), window: (usize, usize)) -> Fit {
        let (gw, gh) = (guest.0.max(1) as f32, guest.1.max(1) as f32);
        let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
        let scale = (ww / gw).min(wh / gh);
        let (width, height) = (gw * scale, gh * scale);
        Fit {
            x: (ww - width) / 2.0,
            y: (wh - height) / 2.0,
            width,
            height,
        }
    }

    /// The guest pixel under a window position, or None on the bars.
    pub fn to_guest(&self, guest: (u32, u32), at: (f32, f32)) -> Option<(i32, i32)> {
        let (fx, fy) = ((at.0 - self.x) / self.width, (at.1 - self.y) / self.height);
        if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
            return None;
        }
        Some((
            ((fx * guest.0 as f32) as i32).min(guest.0 as i32 - 1),
            ((fy * guest.1 as f32) as i32).min(guest.1 as i32 - 1),
        ))
    }
}

/// Convert 24-bit RGB bytes to the window's pixels (0x00RRGGBB each).
pub fn rgb_to_window(rgb: &[u8], out: &mut Vec<u32>) {
    out.clear();
    out.extend(
        rgb.chunks_exact(3)
            .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tall_guest_in_a_wide_window_gets_side_bars() {
        let f = Fit::new((720, 1280), (1280, 640));
        assert_eq!((f.width, f.height), (360.0, 640.0));
        assert_eq!((f.x, f.y), (460.0, 0.0));
        assert_eq!(f.to_guest((720, 1280), (460.0, 0.0)), Some((0, 0)));
        assert_eq!(f.to_guest((720, 1280), (640.0, 320.0)), Some((360, 640)));
        assert_eq!(f.to_guest((720, 1280), (819.9, 639.9)), Some((719, 1279)));
        assert_eq!(f.to_guest((720, 1280), (459.0, 10.0)), None, "left bar");
        assert_eq!(f.to_guest((720, 1280), (820.0, 10.0)), None, "right bar");
    }

    #[test]
    fn a_window_of_the_same_shape_has_no_bars() {
        let f = Fit::new((720, 1280), (360, 640));
        assert_eq!(
            f,
            Fit {
                x: 0.0,
                y: 0.0,
                width: 360.0,
                height: 640.0
            }
        );
        assert_eq!(f.to_guest((720, 1280), (180.0, 320.0)), Some((360, 640)));
        assert_eq!(f.to_guest((720, 1280), (-1.0, 5.0)), None);
        // Degenerate sizes do not divide by zero.
        let _ = Fit::new((0, 0), (0, 0)).to_guest((0, 0), (0.0, 0.0));
    }

    #[test]
    fn pixels_are_packed_red_high() {
        let mut out = vec![7];
        rgb_to_window(&[0x11, 0x22, 0x33, 0xff, 0x00, 0x80], &mut out);
        assert_eq!(out, [0x0011_2233, 0x00ff_0080]);
    }
}
