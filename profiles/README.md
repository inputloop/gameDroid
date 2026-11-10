# Game profiles

One TOML file per game, named after the Android package. The format is
described in `core/input/src/profile.rs` and checked by
`gamedroid_input::Profile::parse`.

`example.shooter.toml` is a made-up game used to show the format. Real
profiles will be added once the emulator can run games to tune them against.
