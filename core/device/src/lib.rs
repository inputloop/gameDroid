// SPDX-License-Identifier: GPL-3.0-or-later
//! gameDroid's virtual Android device.
//!
//! [`android`] describes the machine Android runs in (disks, screen, input
//! devices, the ports its services expect) on top of the bvmm engine.
//! [`cuttlefish`] supplies the host's side of conversations that the stock
//! Android virtual-device image has with helpers it expects on the host.

pub mod android;
pub mod cuttlefish;
