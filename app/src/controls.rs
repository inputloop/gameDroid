// SPDX-License-Identifier: GPL-3.0-or-later
//! The host's keys and buttons: what profiles call them, and which Linux
//! key code Android should see when a key is typed straight through.

use minifb::Key;

/// The name a profile uses for a key, such as `W` or `Space`.
pub fn key_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Key0 => "0",
        Key::Key1 => "1",
        Key::Key2 => "2",
        Key::Key3 => "3",
        Key::Key4 => "4",
        Key::Key5 => "5",
        Key::Key6 => "6",
        Key::Key7 => "7",
        Key::Key8 => "8",
        Key::Key9 => "9",
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::LeftShift => "LeftShift",
        Key::RightShift => "RightShift",
        Key::LeftCtrl => "LeftCtrl",
        Key::RightCtrl => "RightCtrl",
        Key::LeftAlt => "LeftAlt",
        _ => return None,
    })
}

/// Names for the mouse buttons, usable in profiles like any key.
pub const MOUSE_LEFT: &str = "MouseLeft";
pub const MOUSE_RIGHT: &str = "MouseRight";
pub const MOUSE_MIDDLE: &str = "MouseMiddle";

/// Android's Back and Home, as Linux key codes.
pub const KEY_BACK: u16 = 158;
pub const KEY_HOME: u16 = 172;

/// The Linux key code for a key typed straight through to Android.
pub fn linux_code(key: Key) -> Option<u16> {
    Some(match key {
        Key::Key1 => 2,
        Key::Key2 => 3,
        Key::Key3 => 4,
        Key::Key4 => 5,
        Key::Key5 => 6,
        Key::Key6 => 7,
        Key::Key7 => 8,
        Key::Key8 => 9,
        Key::Key9 => 10,
        Key::Key0 => 11,
        Key::Minus => 12,
        Key::Equal => 13,
        Key::Backspace => 14,
        Key::Tab => 15,
        Key::Q => 16,
        Key::W => 17,
        Key::E => 18,
        Key::R => 19,
        Key::T => 20,
        Key::Y => 21,
        Key::U => 22,
        Key::I => 23,
        Key::O => 24,
        Key::P => 25,
        Key::Enter => 28,
        Key::LeftCtrl => 29,
        Key::A => 30,
        Key::S => 31,
        Key::D => 32,
        Key::F => 33,
        Key::G => 34,
        Key::H => 35,
        Key::J => 36,
        Key::K => 37,
        Key::L => 38,
        Key::LeftShift => 42,
        Key::Z => 44,
        Key::X => 45,
        Key::C => 46,
        Key::V => 47,
        Key::B => 48,
        Key::N => 49,
        Key::M => 50,
        Key::Comma => 51,
        Key::Period => 52,
        Key::Slash => 53,
        Key::RightShift => 54,
        Key::Space => 57,
        Key::Up => 103,
        Key::Left => 105,
        Key::Right => 106,
        Key::Down => 108,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_example_profile() {
        // The profile shipped in profiles/ uses these.
        for (key, name) in [
            (Key::W, "W"),
            (Key::A, "A"),
            (Key::Space, "Space"),
            (Key::R, "R"),
        ] {
            assert_eq!(key_name(key), Some(name));
        }
        assert_eq!(key_name(Key::F1), None, "function keys belong to gameDroid");
    }

    #[test]
    fn codes_follow_the_linux_keyboard_layout() {
        assert_eq!(linux_code(Key::A), Some(30));
        assert_eq!(linux_code(Key::Q), Some(16));
        assert_eq!(linux_code(Key::Key1), Some(2));
        assert_eq!(linux_code(Key::Key0), Some(11));
        assert_eq!(linux_code(Key::Enter), Some(28));
        assert_eq!(linux_code(Key::Space), Some(57));
        assert_eq!(linux_code(Key::F1), None);
        // No two keys share a code.
        let all = [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
            Key::Key0,
            Key::Key1,
            Key::Key2,
            Key::Key3,
            Key::Key4,
            Key::Key5,
            Key::Key6,
            Key::Key7,
            Key::Key8,
            Key::Key9,
            Key::Space,
            Key::Enter,
            Key::Tab,
            Key::Backspace,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::LeftShift,
            Key::RightShift,
            Key::LeftCtrl,
            Key::Minus,
            Key::Equal,
            Key::Comma,
            Key::Period,
            Key::Slash,
        ];
        let mut codes: Vec<u16> = all.iter().filter_map(|k| linux_code(*k)).collect();
        assert_eq!(codes.len(), all.len());
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len());
    }
}
