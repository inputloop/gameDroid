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

/// Capability sets a guest may be offered (virtio-gpu's numbering).
const CAPSETS: [(u32, &str); 3] = [
    (3, "gfxstream-vulkan"),
    (8, "gfxstream-gles"),
    (9, "gfxstream-composer"),
];

fn main() -> ExitCode {
    let Some(library) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gamedroid-gpu-check <gfxstream library>");
        return ExitCode::from(2);
    };
    let renderer = match bvmm::gfxstream::Gfxstream::start(&library, 720, 1280, Box::new(|_| {})) {
        Ok(renderer) => renderer,
        Err(e) => {
            println!("GPU_CHECK=FAIL {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut vulkan = false;
    for (id, name) in CAPSETS {
        let (version, size) = renderer.capset_info(id);
        let bytes = renderer.capset(id, version);
        println!(
            "capability set {id} {name}: {} (version {version}, {size} bytes, {} non-zero)",
            if size > 0 { "offered" } else { "not offered" },
            bytes.iter().filter(|b| **b != 0).count()
        );
        vulkan |= id == 3 && size > 0;
    }
    let context = renderer.context_create(1, "gamedroid-gpu-check", 3);
    println!(
        "a Vulkan context: {}",
        if context.is_ok() {
            "created"
        } else {
            "refused"
        }
    );
    if context.is_ok() {
        renderer.context_destroy(1);
    }
    drop(renderer);
    if vulkan && context.is_ok() {
        println!("GPU_CHECK=PASS");
        ExitCode::SUCCESS
    } else {
        println!("GPU_CHECK=FAIL the renderer started but does not offer Vulkan");
        ExitCode::FAILURE
    }
}
