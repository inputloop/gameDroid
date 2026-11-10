// SPDX-License-Identifier: GPL-3.0-or-later
//! Applies a profile: host input in, touch actions out.
//!
//! The engine keeps track of which fingers are down so that every `Down` is
//! eventually followed by an `Up` for the same finger, whatever order keys
//! are pressed and released in. It never reads a clock; the caller says
//! when the pointer has gone idle.

use crate::profile::{Binding, Profile, MAX_TOUCHES};

/// What the player did on the host.
#[derive(Debug, Clone, PartialEq)]
pub enum HostInput<'a> {
    /// A named control (key, mouse button, controller button) went down or up.
    Control { name: &'a str, down: bool },
    /// The pointer moved by this many host pixels.
    Pointer { dx: f64, dy: f64 },
    /// The pointer has stopped for a while: let go of the look finger.
    PointerIdle,
    /// Let go of everything (focus lost, profile switched, game closed).
    ReleaseAll,
}

/// One thing a finger does on the guest's touchscreen, in guest pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    Down { finger: u32, x: i32, y: i32 },
    Move { finger: u32, x: i32, y: i32 },
    Up { finger: u32 },
}

#[derive(Debug, Clone)]
enum State {
    Tap,
    /// Which of up, down, left, right are held.
    Joystick {
        held: [bool; 4],
    },
    /// Where the finger is, in pixels, while it is down.
    Look {
        at: (f64, f64),
    },
}

#[derive(Debug, Clone)]
struct Bound {
    binding: Binding,
    state: State,
    /// The finger this binding currently has down.
    finger: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Engine {
    width: f64,
    height: f64,
    bound: Vec<Bound>,
    fingers: [bool; MAX_TOUCHES],
}

impl Engine {
    /// An engine for `profile` on a screen of `width` by `height` pixels.
    pub fn new(profile: &Profile, width: u32, height: u32) -> Engine {
        let bound = profile
            .bindings
            .iter()
            .map(|b| Bound {
                binding: b.clone(),
                state: match b {
                    Binding::Tap { .. } => State::Tap,
                    Binding::Joystick { .. } => State::Joystick { held: [false; 4] },
                    Binding::Look { .. } => State::Look { at: (0.0, 0.0) },
                },
                finger: None,
            })
            .collect();
        Engine {
            width: width.max(1) as f64,
            height: height.max(1) as f64,
            bound,
            fingers: [false; MAX_TOUCHES],
        }
    }

    /// How many fingers are down right now.
    pub fn fingers_down(&self) -> usize {
        self.fingers.iter().filter(|f| **f).count()
    }

    fn round(p: (f64, f64)) -> (i32, i32) {
        (p.0.round() as i32, p.1.round() as i32)
    }

    fn take_finger(fingers: &mut [bool; MAX_TOUCHES]) -> Option<u32> {
        let free = fingers.iter().position(|f| !*f)?;
        fingers[free] = true;
        Some(free as u32)
    }

    /// Feed one host input; get the touches it causes, in order.
    pub fn input(&mut self, input: HostInput) -> Vec<Touch> {
        match input {
            HostInput::Control { name, down } => self.control(name, down),
            HostInput::Pointer { dx, dy } => self.pointer(dx, dy),
            HostInput::PointerIdle => self.release(|b| matches!(b.state, State::Look { .. })),
            HostInput::ReleaseAll => {
                for b in &mut self.bound {
                    if let State::Joystick { held } = &mut b.state {
                        *held = [false; 4];
                    }
                }
                self.release(|_| true)
            }
        }
    }

    fn release(&mut self, which: impl Fn(&Bound) -> bool) -> Vec<Touch> {
        let mut out = Vec::new();
        for b in &mut self.bound {
            if which(b) {
                if let Some(finger) = b.finger.take() {
                    self.fingers[finger as usize] = false;
                    out.push(Touch::Up { finger });
                }
            }
        }
        out
    }

    fn control(&mut self, name: &str, down: bool) -> Vec<Touch> {
        let mut out = Vec::new();
        let (w, h) = (self.width, self.height);
        let px = |x: f64, y: f64| (x * (w - 1.0), y * (h - 1.0));
        for b in &mut self.bound {
            match (&b.binding, &mut b.state) {
                (Binding::Tap { key, x, y }, State::Tap) if key == name => {
                    match (down, b.finger) {
                        (true, None) => {
                            if let Some(finger) = Engine::take_finger(&mut self.fingers) {
                                b.finger = Some(finger);
                                let (x, y) = Engine::round(px(*x, *y));
                                out.push(Touch::Down { finger, x, y });
                            }
                        }
                        (false, Some(finger)) => {
                            b.finger = None;
                            self.fingers[finger as usize] = false;
                            out.push(Touch::Up { finger });
                        }
                        // A repeated press, or a release of something not held.
                        _ => {}
                    }
                }
                (
                    Binding::Joystick {
                        up,
                        down: down_key,
                        left,
                        right,
                        x,
                        y,
                        radius,
                    },
                    State::Joystick { held },
                ) => {
                    let Some(which) = [up, down_key, left, right].iter().position(|k| *k == name)
                    else {
                        continue;
                    };
                    if held[which] == down {
                        continue;
                    }
                    held[which] = down;
                    let dx = held[3] as i32 - held[2] as i32;
                    let dy = held[1] as i32 - held[0] as i32;
                    let centre = px(*x, *y);
                    if dx == 0 && dy == 0 {
                        if let Some(finger) = b.finger.take() {
                            self.fingers[finger as usize] = false;
                            out.push(Touch::Up { finger });
                        }
                        continue;
                    }
                    // The stick's reach is a fraction of the shorter side, so
                    // it is a circle on any screen; a diagonal is no longer
                    // than a straight push.
                    let reach = radius * w.min(h);
                    let len = ((dx * dx + dy * dy) as f64).sqrt();
                    let target = (
                        (centre.0 + dx as f64 / len * reach).clamp(0.0, w - 1.0),
                        (centre.1 + dy as f64 / len * reach).clamp(0.0, h - 1.0),
                    );
                    let (tx, ty) = Engine::round(target);
                    match b.finger {
                        Some(finger) => out.push(Touch::Move {
                            finger,
                            x: tx,
                            y: ty,
                        }),
                        None => {
                            if let Some(finger) = Engine::take_finger(&mut self.fingers) {
                                b.finger = Some(finger);
                                // Down at the centre, then drag: games read
                                // a stick as a drag away from where it began.
                                let (cx, cy) = Engine::round(centre);
                                out.push(Touch::Down {
                                    finger,
                                    x: cx,
                                    y: cy,
                                });
                                out.push(Touch::Move {
                                    finger,
                                    x: tx,
                                    y: ty,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn pointer(&mut self, dx: f64, dy: f64) -> Vec<Touch> {
        let mut out = Vec::new();
        if !(dx.is_finite() && dy.is_finite()) || (dx == 0.0 && dy == 0.0) {
            return out;
        }
        let (w, h) = (self.width, self.height);
        for b in &mut self.bound {
            let (
                Binding::Look {
                    x,
                    y,
                    width,
                    height,
                    sensitivity,
                },
                State::Look { at },
            ) = (&b.binding, &mut b.state)
            else {
                continue;
            };
            let centre = (x * (w - 1.0), y * (h - 1.0));
            let half = (width * (w - 1.0) / 2.0, height * (h - 1.0) / 2.0);
            let (min, max) = (
                (centre.0 - half.0, centre.1 - half.1),
                (centre.0 + half.0, centre.1 + half.1),
            );
            let finger = match b.finger {
                Some(f) => f,
                None => {
                    let Some(f) = Engine::take_finger(&mut self.fingers) else {
                        continue;
                    };
                    b.finger = Some(f);
                    *at = centre;
                    let (cx, cy) = Engine::round(centre);
                    out.push(Touch::Down {
                        finger: f,
                        x: cx,
                        y: cy,
                    });
                    f
                }
            };
            let wanted = (at.0 + dx * sensitivity, at.1 + dy * sensitivity);
            let clamped = (wanted.0.clamp(min.0, max.0), wanted.1.clamp(min.1, max.1));
            *at = clamped;
            let (mx, my) = Engine::round(clamped);
            out.push(Touch::Move {
                finger,
                x: mx,
                y: my,
            });
            if clamped != wanted {
                // Ran out of room: lift and start again from the centre, as
                // a thumb does. The lift does not turn the camera.
                *at = centre;
                let (cx, cy) = Engine::round(centre);
                out.push(Touch::Up { finger });
                out.push(Touch::Down {
                    finger,
                    x: cx,
                    y: cy,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(bindings: &str) -> Engine {
        let text = format!("schema = 1\npackage = \"a.b\"\n{bindings}");
        Engine::new(&Profile::parse(&text).unwrap(), 1001, 501)
    }

    fn key<'a>(name: &'a str, down: bool) -> HostInput<'a> {
        HostInput::Control { name, down }
    }

    const TAPS: &str = "[[bind]]\ntype=\"tap\"\nkey=\"Space\"\nx=0.9\ny=0.8\n\
                        [[bind]]\ntype=\"tap\"\nkey=\"E\"\nx=0.5\ny=0.5\n";
    const STICK: &str = "[[bind]]\ntype=\"joystick\"\nup=\"W\"\ndown=\"S\"\nleft=\"A\"\nright=\"D\"\nx=0.2\ny=0.6\nradius=0.1\n";
    const LOOK: &str =
        "[[bind]]\ntype=\"look\"\nx=0.7\ny=0.5\nwidth=0.4\nheight=0.4\nsensitivity=2.0\n";

    #[test]
    fn a_tap_holds_while_the_key_is_held() {
        let mut e = engine(TAPS);
        assert_eq!(
            e.input(key("Space", true)),
            [Touch::Down {
                finger: 0,
                x: 900,
                y: 400
            }]
        );
        assert!(e.input(key("Space", true)).is_empty(), "key repeat");
        assert!(e.input(key("Q", true)).is_empty(), "unbound key");
        assert_eq!(e.input(key("Space", false)), [Touch::Up { finger: 0 }]);
        assert!(e.input(key("Space", false)).is_empty(), "already up");
        assert_eq!(e.fingers_down(), 0);
    }

    #[test]
    fn two_taps_use_two_fingers_and_free_them_independently() {
        let mut e = engine(TAPS);
        assert_eq!(
            e.input(key("Space", true)),
            [Touch::Down {
                finger: 0,
                x: 900,
                y: 400
            }]
        );
        assert_eq!(
            e.input(key("E", true)),
            [Touch::Down {
                finger: 1,
                x: 500,
                y: 250
            }]
        );
        assert_eq!(e.input(key("Space", false)), [Touch::Up { finger: 0 }]);
        // The freed finger is reused; the other is still down.
        assert_eq!(
            e.input(key("Space", true)),
            [Touch::Down {
                finger: 0,
                x: 900,
                y: 400
            }]
        );
        assert_eq!(e.fingers_down(), 2);
    }

    #[test]
    fn a_joystick_drags_from_its_centre() {
        let mut e = engine(STICK);
        // Centre (200, 300); reach is 0.1 of the shorter side, 50 px.
        assert_eq!(
            e.input(key("W", true)),
            [
                Touch::Down {
                    finger: 0,
                    x: 200,
                    y: 300
                },
                Touch::Move {
                    finger: 0,
                    x: 200,
                    y: 250
                }
            ]
        );
        // Adding D makes a diagonal of the same length: 50/sqrt(2) = 35.36.
        assert_eq!(
            e.input(key("D", true)),
            [Touch::Move {
                finger: 0,
                x: 235,
                y: 265
            }]
        );
        assert_eq!(
            e.input(key("W", false)),
            [Touch::Move {
                finger: 0,
                x: 250,
                y: 300
            }]
        );
        assert_eq!(e.input(key("D", false)), [Touch::Up { finger: 0 }]);
        assert_eq!(e.fingers_down(), 0);
    }

    #[test]
    fn opposite_keys_cancel() {
        let mut e = engine(STICK);
        e.input(key("A", true));
        // Left and right together: the stick is centred, so the finger lifts.
        assert_eq!(e.input(key("D", true)), [Touch::Up { finger: 0 }]);
        // Releasing one leaves the other held: the stick goes that way again.
        assert_eq!(
            e.input(key("A", false)),
            [
                Touch::Down {
                    finger: 0,
                    x: 200,
                    y: 300
                },
                Touch::Move {
                    finger: 0,
                    x: 250,
                    y: 300
                }
            ]
        );
    }

    #[test]
    fn the_pointer_drags_inside_the_look_area_and_recentres_at_the_edge() {
        let mut e = engine(LOOK);
        // Centre (700, 250); area 400 x 200, so x in 500..900 and y in 150..350.
        assert_eq!(
            e.input(HostInput::Pointer { dx: 10.0, dy: -5.0 }),
            [
                Touch::Down {
                    finger: 0,
                    x: 700,
                    y: 250
                },
                Touch::Move {
                    finger: 0,
                    x: 720,
                    y: 240
                }
            ]
        );
        assert_eq!(
            e.input(HostInput::Pointer { dx: 30.0, dy: 0.0 }),
            [Touch::Move {
                finger: 0,
                x: 780,
                y: 240
            }]
        );
        // 80 more host pixels is 160 guest pixels: past the right edge at 900.
        assert_eq!(
            e.input(HostInput::Pointer { dx: 80.0, dy: 0.0 }),
            [
                Touch::Move {
                    finger: 0,
                    x: 900,
                    y: 240
                },
                Touch::Up { finger: 0 },
                Touch::Down {
                    finger: 0,
                    x: 700,
                    y: 250
                }
            ]
        );
        assert_eq!(e.fingers_down(), 1);
        assert_eq!(e.input(HostInput::PointerIdle), [Touch::Up { finger: 0 }]);
        assert!(e.input(HostInput::PointerIdle).is_empty());
        assert!(e.input(HostInput::Pointer { dx: 0.0, dy: 0.0 }).is_empty());
        assert!(e
            .input(HostInput::Pointer {
                dx: f64::NAN,
                dy: 1.0
            })
            .is_empty());
    }

    #[test]
    fn everything_at_once_then_release_all() {
        let mut e = engine(&format!("{TAPS}{STICK}{LOOK}"));
        e.input(key("W", true));
        e.input(key("Space", true));
        e.input(HostInput::Pointer { dx: 1.0, dy: 1.0 });
        assert_eq!(e.fingers_down(), 3);
        let ups = e.input(HostInput::ReleaseAll);
        assert_eq!(ups.len(), 3);
        assert!(ups.iter().all(|t| matches!(t, Touch::Up { .. })));
        assert_eq!(e.fingers_down(), 0);
        // The stick forgot W was held: releasing it now does nothing, and
        // pressing D alone goes right, not up-right.
        assert!(e.input(key("W", false)).is_empty());
        assert_eq!(
            e.input(key("D", true)),
            [
                Touch::Down {
                    finger: 0,
                    x: 200,
                    y: 300
                },
                Touch::Move {
                    finger: 0,
                    x: 250,
                    y: 300
                }
            ]
        );
    }

    /// Whatever the player does, fingers never leak: every Down has its Up,
    /// no finger goes down twice, and nothing moves while up.
    #[test]
    fn fingers_never_leak_under_arbitrary_input() {
        let mut e = engine(&format!("{TAPS}{STICK}{LOOK}"));
        let names = ["Space", "E", "W", "A", "S", "D", "Q"];
        let mut down = [false; MAX_TOUCHES];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let r = next();
            let input = match r % 10 {
                0..=5 => key(names[(r >> 8) as usize % names.len()], (r >> 16) & 1 == 1),
                6..=7 => HostInput::Pointer {
                    dx: ((r >> 8) % 200) as f64 - 100.0,
                    dy: ((r >> 20) % 200) as f64 - 100.0,
                },
                8 => HostInput::PointerIdle,
                _ => HostInput::ReleaseAll,
            };
            for t in e.input(input) {
                match t {
                    Touch::Down { finger, x, y } => {
                        assert!(!down[finger as usize], "finger {finger} went down twice");
                        assert!((0..1001).contains(&x) && (0..501).contains(&y));
                        down[finger as usize] = true;
                    }
                    Touch::Move { finger, x, y } => {
                        assert!(down[finger as usize], "finger {finger} moved while up");
                        assert!((0..1001).contains(&x) && (0..501).contains(&y));
                    }
                    Touch::Up { finger } => {
                        assert!(down[finger as usize], "finger {finger} lifted while up");
                        down[finger as usize] = false;
                    }
                }
            }
            assert_eq!(e.fingers_down(), down.iter().filter(|d| **d).count());
        }
        for t in e.input(HostInput::ReleaseAll) {
            if let Touch::Up { finger } = t {
                down[finger as usize] = false;
            }
        }
        assert!(
            down.iter().all(|d| !*d),
            "everything is released at the end"
        );
    }
}
