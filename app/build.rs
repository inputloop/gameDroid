// SPDX-License-Identifier: GPL-3.0-or-later
//! If `GAMEDROID_GFXSTREAM_EMBED` names a built gfxstream library, gameDroid
//! carries it inside itself, so that the single file can draw with the
//! graphics card without anything beside it.

fn main() {
    println!("cargo:rerun-if-env-changed=GAMEDROID_GFXSTREAM_EMBED");
    println!("cargo:rustc-check-cfg=cfg(carries_gfxstream)");
    if let Ok(path) = std::env::var("GAMEDROID_GFXSTREAM_EMBED") {
        if !path.is_empty() {
            assert!(
                std::path::Path::new(&path).is_file(),
                "GAMEDROID_GFXSTREAM_EMBED names no file: {path}"
            );
            println!("cargo:rerun-if-changed={path}");
            println!("cargo:rustc-env=GAMEDROID_GFXSTREAM_PATH={path}");
            println!("cargo:rustc-cfg=carries_gfxstream");
        }
    }
}
