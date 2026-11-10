// SPDX-License-Identifier: GPL-3.0-or-later
//! gameDroid's input engine.
//!
//! A touch-only game is played with a keyboard, mouse or controller by
//! turning what the player does on the host into touches the game would get
//! from fingers. Which key becomes which touch is a [`profile::Profile`],
//! one per game. [`engine::Engine`] applies a profile: host input goes in,
//! touch actions come out. It has no I/O and no clock, so everything it
//! does can be tested exactly.
//!
//! Android only ever sees an ordinary touchscreen. The mapping lives on the
//! host.

pub mod engine;
pub mod profile;

pub use engine::{Engine, HostInput, Touch};
pub use profile::{Binding, Profile, ProfileError};
