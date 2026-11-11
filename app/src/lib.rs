// SPDX-License-Identifier: GPL-3.0-or-later
//! The logic of gameDroid's window, kept apart from the window itself so it
//! can be tested without a screen.
//!
//! * [`view`] fits the guest's picture into the window and maps a point in
//!   the window back to a point on the guest's screen.
//! * [`controls`] names the host's keys and buttons, for profiles, and gives
//!   the Linux key codes Android expects for typing.
//! * [`frontend`] turns what the player is doing this instant into touches
//!   and key presses for the guest.

pub mod audio;
pub mod controls;
pub mod frontend;
pub mod view;
