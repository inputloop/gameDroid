// SPDX-License-Identifier: GPL-3.0-or-later
//! Start the GPU renderer and say what it offers, then stop.
//!
//! This is the check gameDroid runs in a separate process before it trusts
//! the renderer with a real session: a renderer that cannot start on this
//! machine (no Vulkan driver, say) has been seen to hang or crash instead
//! of reporting an error, and a separate process can be abandoned.
//!
//! Exit status 0 means the renderer started and offers Vulkan to a guest.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(library) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gamedroid-gpu-check <gfxstream library>");
        return ExitCode::from(2);
    };
    match gamedroid_device::gpu::check_here(&library) {
        Ok(()) => {
            println!("GPU_CHECK=PASS");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("GPU_CHECK=FAIL {e}");
            ExitCode::FAILURE
        }
    }
}
