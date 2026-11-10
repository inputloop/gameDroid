// SPDX-License-Identifier: GPL-3.0-or-later
//! Every profile shipped in the repository must load.

use gamedroid_input::{Engine, HostInput, Profile, Touch};

#[test]
fn shipped_profiles_load_and_drive_an_engine() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../profiles");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let profile = Profile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut engine = Engine::new(&profile, 1920, 1080);
        let ups = engine.input(HostInput::ReleaseAll);
        assert!(ups.is_empty(), "a fresh engine has nothing down");
        seen += 1;
    }
    assert!(seen >= 1, "no profiles found in {dir}");
}

#[test]
fn the_example_shooter_plays() {
    let text = include_str!("../../../profiles/example.shooter.toml");
    let mut e = Engine::new(&Profile::parse(text).unwrap(), 1920, 1080);
    // Hold W and fire while turning: three fingers.
    let w = e.input(HostInput::Control {
        name: "W",
        down: true,
    });
    assert!(matches!(w[0], Touch::Down { .. }) && matches!(w[1], Touch::Move { .. }));
    e.input(HostInput::Control {
        name: "MouseLeft",
        down: true,
    });
    e.input(HostInput::Pointer { dx: 12.0, dy: 3.0 });
    assert_eq!(e.fingers_down(), 3);
    assert_eq!(e.input(HostInput::ReleaseAll).len(), 3);
}
