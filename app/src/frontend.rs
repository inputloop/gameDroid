// SPDX-License-Identifier: GPL-3.0-or-later
//! From what the player is doing this instant to what the guest should get.
//!
//! Two modes:
//!
//! * **Direct**: the mouse is a finger (press and drag on the picture) and
//!   the keyboard types into Android.
//! * **Game**: keys, mouse buttons and mouse movement go through the game's
//!   profile and come out as touches. Only available when a profile is
//!   loaded. F1 switches between the two.
//!
//! In both modes Escape is Android's Back and F2 is Home.

use gamedroid_input::{Engine, HostInput, Profile, Touch};
use minifb::Key;

use crate::controls::{
    key_name, linux_code, KEY_BACK, KEY_HOME, MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT,
};
use crate::view::Fit;

/// The finger the mouse uses in direct mode: the last one, so it stays out
/// of the way of fingers a profile uses.
pub const MOUSE_FINGER: u32 = 9;
/// After this many frames without pointer movement, the look finger lifts.
const IDLE_FRAMES: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Direct,
    Game,
}

/// A snapshot of the host's input devices and window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostState {
    pub keys: Vec<Key>,
    /// Pointer position in window pixels, if it is over the window.
    pub mouse: Option<(f32, f32)>,
    /// Left, right, middle.
    pub buttons: [bool; 3],
    pub window: (usize, usize),
}

/// Something to send to the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Touch(Touch),
    Key { code: u16, down: bool },
}

pub struct Frontend {
    guest: (u32, u32),
    mode: Mode,
    engine: Option<Engine>,
    before: HostState,
    mouse_finger_down: bool,
    still_frames: u32,
    /// Key codes currently held down in Android by direct typing.
    typed: Vec<u16>,
}

impl Frontend {
    pub fn new(guest: (u32, u32), profile: Option<&Profile>) -> Frontend {
        Frontend {
            guest,
            mode: Mode::Direct,
            engine: profile.map(|p| Engine::new(p, guest.0, guest.1)),
            before: HostState::default(),
            mouse_finger_down: false,
            still_frames: 0,
            typed: Vec::new(),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn has_profile(&self) -> bool {
        self.engine.is_some()
    }

    fn touches(out: &mut Vec<Action>, touches: Vec<Touch>) {
        out.extend(touches.into_iter().map(Action::Touch));
    }

    fn lift_mouse_finger(&mut self, out: &mut Vec<Action>) {
        if std::mem::take(&mut self.mouse_finger_down) {
            out.push(Action::Touch(Touch::Up {
                finger: MOUSE_FINGER,
            }));
        }
    }

    /// Let go of everything the player was holding in the current mode.
    fn release_all(&mut self, out: &mut Vec<Action>) {
        self.lift_mouse_finger(out);
        if let Some(engine) = &mut self.engine {
            Frontend::touches(out, engine.input(HostInput::ReleaseAll));
        }
        for code in self.typed.drain(..) {
            out.push(Action::Key { code, down: false });
        }
    }

    /// Advance by one frame.
    pub fn step(&mut self, now: &HostState) -> Vec<Action> {
        let mut out = Vec::new();
        let pressed: Vec<Key> = now
            .keys
            .iter()
            .filter(|k| !self.before.keys.contains(k))
            .copied()
            .collect();
        let released: Vec<Key> = self
            .before
            .keys
            .iter()
            .filter(|k| !now.keys.contains(k))
            .copied()
            .collect();

        // Back and Home work the same in both modes, and are handled before
        // a mode switch so a release in the same frame is not lost.
        for (keys, down) in [(&pressed, true), (&released, false)] {
            for key in keys {
                match key {
                    Key::Escape => out.push(Action::Key {
                        code: KEY_BACK,
                        down,
                    }),
                    Key::F2 => out.push(Action::Key {
                        code: KEY_HOME,
                        down,
                    }),
                    _ => {}
                }
            }
        }

        if pressed.contains(&Key::F1) && self.engine.is_some() {
            self.release_all(&mut out);
            self.mode = match self.mode {
                Mode::Direct => Mode::Game,
                Mode::Game => Mode::Direct,
            };
            // Keys held across the switch are not pressed again in the new mode.
            self.before = now.clone();
            return out;
        }
        let fit = Fit::new(self.guest, now.window);
        match self.mode {
            Mode::Direct => {
                // Only keys that went down in this mode come up in it: a key
                // held across a switch from game mode was never typed.
                for code in pressed.iter().filter_map(|k| linux_code(*k)) {
                    if !self.typed.contains(&code) {
                        self.typed.push(code);
                        out.push(Action::Key { code, down: true });
                    }
                }
                for code in released.iter().filter_map(|k| linux_code(*k)) {
                    if let Some(i) = self.typed.iter().position(|c| *c == code) {
                        self.typed.remove(i);
                        out.push(Action::Key { code, down: false });
                    }
                }
                let at = now
                    .mouse
                    .filter(|_| now.buttons[0])
                    .and_then(|m| fit.to_guest(self.guest, m));
                match at {
                    Some((x, y)) => {
                        let moved = now.mouse != self.before.mouse;
                        if !self.mouse_finger_down {
                            self.mouse_finger_down = true;
                            out.push(Action::Touch(Touch::Down {
                                finger: MOUSE_FINGER,
                                x,
                                y,
                            }));
                        } else if moved {
                            out.push(Action::Touch(Touch::Move {
                                finger: MOUSE_FINGER,
                                x,
                                y,
                            }));
                        }
                    }
                    None => self.lift_mouse_finger(&mut out),
                }
            }
            Mode::Game => {
                let Some(engine) = &mut self.engine else {
                    self.before = now.clone();
                    return out;
                };
                for (keys, down) in [(&pressed, true), (&released, false)] {
                    for name in keys.iter().filter_map(|k| key_name(*k)) {
                        Frontend::touches(
                            &mut out,
                            engine.input(HostInput::Control { name, down }),
                        );
                    }
                }
                for (i, name) in [MOUSE_LEFT, MOUSE_RIGHT, MOUSE_MIDDLE]
                    .into_iter()
                    .enumerate()
                {
                    if now.buttons[i] != self.before.buttons[i] {
                        let down = now.buttons[i];
                        Frontend::touches(
                            &mut out,
                            engine.input(HostInput::Control { name, down }),
                        );
                    }
                }
                let delta = match (self.before.mouse, now.mouse) {
                    (Some(a), Some(b)) => (b.0 - a.0, b.1 - a.1),
                    _ => (0.0, 0.0),
                };
                if delta != (0.0, 0.0) {
                    self.still_frames = 0;
                    // Window pixels to guest pixels.
                    let scale = self.guest.0 as f32 / fit.width.max(1.0);
                    Frontend::touches(
                        &mut out,
                        engine.input(HostInput::Pointer {
                            dx: (delta.0 * scale) as f64,
                            dy: (delta.1 * scale) as f64,
                        }),
                    );
                } else {
                    self.still_frames += 1;
                    if self.still_frames == IDLE_FRAMES {
                        Frontend::touches(&mut out, engine.input(HostInput::PointerIdle));
                    }
                }
            }
        }
        self.before = now.clone();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUEST: (u32, u32) = (720, 1280);

    fn state(keys: &[Key], mouse: Option<(f32, f32)>, left: bool) -> HostState {
        HostState {
            keys: keys.to_vec(),
            mouse,
            buttons: [left, false, false],
            window: (360, 640), // half size, same shape: window x2 = guest
        }
    }

    fn profile() -> Profile {
        Profile::parse(include_str!("../../profiles/example.shooter.toml")).unwrap()
    }

    #[test]
    fn in_direct_mode_the_mouse_is_a_finger() {
        let mut f = Frontend::new(GUEST, None);
        assert!(
            f.step(&state(&[], Some((100.0, 200.0)), false)).is_empty(),
            "hovering"
        );
        assert_eq!(
            f.step(&state(&[], Some((100.0, 200.0)), true)),
            [Action::Touch(Touch::Down {
                finger: 9,
                x: 200,
                y: 400
            })]
        );
        assert!(
            f.step(&state(&[], Some((100.0, 200.0)), true)).is_empty(),
            "held still"
        );
        assert_eq!(
            f.step(&state(&[], Some((110.0, 150.0)), true)),
            [Action::Touch(Touch::Move {
                finger: 9,
                x: 220,
                y: 300
            })]
        );
        assert_eq!(
            f.step(&state(&[], Some((110.0, 150.0)), false)),
            [Action::Touch(Touch::Up { finger: 9 })]
        );
        // Dragging off the window lifts the finger too.
        f.step(&state(&[], Some((10.0, 10.0)), true));
        assert_eq!(
            f.step(&state(&[], None, true)),
            [Action::Touch(Touch::Up { finger: 9 })]
        );
    }

    #[test]
    fn in_direct_mode_keys_type_and_escape_is_back() {
        let mut f = Frontend::new(GUEST, None);
        assert_eq!(
            f.step(&state(&[Key::A], None, false)),
            [Action::Key {
                code: 30,
                down: true
            }]
        );
        assert!(f.step(&state(&[Key::A], None, false)).is_empty(), "held");
        assert_eq!(
            f.step(&state(&[Key::Escape], None, false)),
            [
                Action::Key {
                    code: KEY_BACK,
                    down: true
                },
                Action::Key {
                    code: 30,
                    down: false
                }
            ]
        );
        assert_eq!(
            f.step(&state(&[Key::F2], None, false)),
            [
                Action::Key {
                    code: KEY_HOME,
                    down: true
                },
                Action::Key {
                    code: KEY_BACK,
                    down: false
                }
            ]
        );
        // F1 does nothing without a profile.
        f.step(&state(&[], None, false));
        assert!(f.step(&state(&[Key::F1], None, false)).is_empty());
        assert_eq!(f.mode(), Mode::Direct);
    }

    #[test]
    fn game_mode_sends_keys_through_the_profile() {
        let p = profile();
        let mut f = Frontend::new(GUEST, Some(&p));
        f.step(&state(&[Key::F1], None, false));
        assert_eq!(f.mode(), Mode::Game);
        f.step(&state(&[], None, false));
        // W pushes the stick up: a finger goes down and is dragged.
        let w = f.step(&state(&[Key::W], Some((50.0, 50.0)), false));
        assert_eq!(w.len(), 2);
        assert!(matches!(w[0], Action::Touch(Touch::Down { finger: 0, .. })));
        assert!(matches!(w[1], Action::Touch(Touch::Move { finger: 0, .. })));
        // Nothing is typed into Android in game mode.
        assert!(!w.iter().any(|a| matches!(a, Action::Key { .. })));
        // The left button is bound to "fire" in the example profile.
        let fire = f.step(&state(&[Key::W], Some((50.0, 50.0)), true));
        assert_eq!(fire.len(), 1);
        assert!(matches!(
            fire[0],
            Action::Touch(Touch::Down { finger: 1, .. })
        ));
        // Moving the mouse turns the camera: a third finger, scaled x2.
        let look = f.step(&state(&[Key::W], Some((55.0, 50.0)), true));
        assert!(matches!(
            look[0],
            Action::Touch(Touch::Down { finger: 2, .. })
        ));
        match (look[0], look[1]) {
            (
                Action::Touch(Touch::Down { x: x0, .. }),
                Action::Touch(Touch::Move { x: x1, .. }),
            ) => {
                assert_eq!(x1 - x0, 10, "5 window pixels are 10 guest pixels")
            }
            other => panic!("{other:?}"),
        }
        // Leaving game mode lets go of all three fingers.
        let ups = f.step(&state(&[Key::W, Key::F1], Some((55.0, 50.0)), true));
        assert_eq!(ups.len(), 3);
        assert!(ups
            .iter()
            .all(|a| matches!(a, Action::Touch(Touch::Up { .. }))));
        assert_eq!(f.mode(), Mode::Direct);
        // W is still held from before the switch: it must not start typing.
        assert!(f
            .step(&state(&[Key::W], Some((55.0, 50.0)), false))
            .is_empty());
    }

    #[test]
    fn the_look_finger_lifts_when_the_mouse_rests() {
        let p = profile();
        let mut f = Frontend::new(GUEST, Some(&p));
        f.step(&state(&[Key::F1], Some((50.0, 50.0)), false));
        f.step(&state(&[], Some((50.0, 50.0)), false));
        assert_eq!(f.step(&state(&[], Some((60.0, 50.0)), false)).len(), 2);
        let mut lifted = None;
        for frame in 1..=30 {
            let out = f.step(&state(&[], Some((60.0, 50.0)), false));
            if !out.is_empty() {
                lifted = Some((frame, out));
                break;
            }
        }
        let (frame, out) = lifted.expect("the finger lifts");
        assert_eq!(frame, IDLE_FRAMES);
        assert!(matches!(out[0], Action::Touch(Touch::Up { .. })));
    }

    /// Whatever the player does, every finger that goes down comes up, and
    /// every key pressed is released, once they let go of everything.
    #[test]
    fn nothing_is_left_held_after_arbitrary_input() {
        let p = profile();
        let mut f = Frontend::new(GUEST, Some(&p));
        let keys = [
            Key::W,
            Key::A,
            Key::S,
            Key::D,
            Key::Space,
            Key::R,
            Key::F1,
            Key::Escape,
            Key::F2,
            Key::Q,
        ];
        let mut fingers = [false; 10];
        let mut held = std::collections::BTreeSet::new();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let apply = |actions: Vec<Action>,
                     fingers: &mut [bool; 10],
                     held: &mut std::collections::BTreeSet<u16>| {
            for a in actions {
                match a {
                    Action::Touch(Touch::Down { finger, .. }) => {
                        assert!(!fingers[finger as usize], "finger {finger} down twice");
                        fingers[finger as usize] = true;
                    }
                    Action::Touch(Touch::Move { finger, .. }) => assert!(fingers[finger as usize]),
                    Action::Touch(Touch::Up { finger }) => {
                        assert!(fingers[finger as usize], "finger {finger} up twice");
                        fingers[finger as usize] = false;
                    }
                    Action::Key { code, down: true } => {
                        assert!(held.insert(code), "key {code} down twice")
                    }
                    Action::Key { code, down: false } => {
                        assert!(held.remove(&code), "key {code} up twice")
                    }
                }
            }
        };
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let down: Vec<Key> = keys
                .iter()
                .enumerate()
                .filter(|(i, _)| seed >> i & 1 == 1 && (*i != 6 || seed >> 20 & 7 == 0))
                .map(|(_, k)| *k)
                .collect();
            let mouse = (seed >> 30 & 3 != 0)
                .then_some(((seed >> 32 & 0x1ff) as f32, (seed >> 41 & 0x3ff) as f32));
            let s = HostState {
                keys: down,
                mouse,
                buttons: [seed >> 51 & 1 == 1, seed >> 52 & 1 == 1, false],
                window: (360, 640),
            };
            apply(f.step(&s), &mut fingers, &mut held);
        }
        // The player lets go; give the look finger time to notice.
        for _ in 0..=IDLE_FRAMES {
            apply(
                f.step(&state(&[], Some((1.0, 1.0)), false)),
                &mut fingers,
                &mut held,
            );
        }
        assert_eq!(fingers, [false; 10], "fingers still down");
        assert!(held.is_empty(), "keys still held: {held:?}");
    }
}
