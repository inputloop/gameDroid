// SPDX-License-Identifier: GPL-3.0-or-later
//! gameDroid's virtual Android device.
//!
//! [`android`] describes the machine Android runs in (disks, screen, input
//! devices, the ports its services expect) on top of the bvmm engine.
//! [`cuttlefish`] supplies the host's side of conversations that the stock
//! Android virtual-device image has with helpers it expects on the host.
//! [`script`] is a small input-script format for tests and demonstrations.

pub mod android;
pub mod camera;
pub mod console;
pub mod cuttlefish;
pub mod gpu;
pub mod prepare;
pub mod script;
pub mod setup;
