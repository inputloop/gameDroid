// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-game profiles: which host control does what on the touchscreen.
//!
//! A profile is a TOML file. Positions are fractions of the screen, 0.0 to
//! 1.0 from the top-left corner, so a profile works at any resolution with
//! the same shape of screen.
//!
//! ```toml
//! schema = 1
//! package = "com.example.shooter"
//! name = "Example Shooter"
//!
//! [[bind]]
//! type = "tap"            # touch while the key is held
//! key = "Space"
//! x = 0.90
//! y = 0.80
//!
//! [[bind]]
//! type = "joystick"       # four keys steer a virtual stick
//! up = "W"
//! left = "A"
//! down = "S"
//! right = "D"
//! x = 0.15
//! y = 0.75
//! radius = 0.08           # fraction of the shorter side of the screen
//!
//! [[bind]]
//! type = "look"           # mouse movement drags inside an area
//! x = 0.70
//! y = 0.45
//! width = 0.50
//! height = 0.70
//! sensitivity = 1.0
//! ```

use serde::Deserialize;

/// The profile format this build reads.
pub const SCHEMA: u32 = 1;
/// A touchscreen tracks this many fingers; a profile cannot need more at once.
pub const MAX_TOUCHES: usize = 10;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: u32,
    /// The Android package the profile is for, such as `com.example.game`.
    pub package: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "bind")]
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Binding {
    /// Touch at a point for as long as the control is held.
    Tap { key: String, x: f64, y: f64 },
    /// Four controls steer a virtual stick: a finger that goes down at the
    /// centre and is dragged up to `radius` away in the held direction.
    Joystick {
        up: String,
        down: String,
        left: String,
        right: String,
        x: f64,
        y: f64,
        radius: f64,
    },
    /// Pointer movement drags a finger around inside an area, the way a
    /// thumb turns the camera. When the finger reaches the edge it lifts and
    /// starts again from the centre.
    Look {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default = "one")]
        sensitivity: f64,
    },
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    /// The file is not valid TOML, or has unknown or missing fields.
    Syntax(String),
    /// The file parsed but says something that cannot work.
    Invalid(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Syntax(m) => write!(f, "profile does not parse: {m}"),
            ProfileError::Invalid(m) => write!(f, "profile is not usable: {m}"),
        }
    }
}

impl std::error::Error for ProfileError {}

fn unit(what: &str, v: f64) -> Result<(), ProfileError> {
    if v.is_finite() && (0.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(ProfileError::Invalid(format!(
            "{what} is {v}; positions are fractions of the screen, 0.0 to 1.0"
        )))
    }
}

impl Profile {
    /// Read and check a profile.
    pub fn parse(text: &str) -> Result<Profile, ProfileError> {
        let p: Profile = toml::from_str(text).map_err(|e| ProfileError::Syntax(e.to_string()))?;
        p.check()?;
        Ok(p)
    }

    fn check(&self) -> Result<(), ProfileError> {
        let bad = |m: String| Err(ProfileError::Invalid(m));
        if self.schema != SCHEMA {
            return bad(format!(
                "schema {} is not supported; this build reads schema {SCHEMA}",
                self.schema
            ));
        }
        if self.package.trim().is_empty() {
            return bad("package is empty".into());
        }
        if self.bindings.len() > MAX_TOUCHES {
            return bad(format!(
                "{} bindings, but a touchscreen has {MAX_TOUCHES} fingers",
                self.bindings.len()
            ));
        }
        let mut used: Vec<&str> = Vec::new();
        let mut looks = 0;
        for (i, b) in self.bindings.iter().enumerate() {
            let n = i + 1;
            let keys: Vec<&str> = match b {
                Binding::Tap { key, x, y } => {
                    unit(&format!("binding {n}: x"), *x)?;
                    unit(&format!("binding {n}: y"), *y)?;
                    vec![key]
                }
                Binding::Joystick {
                    up,
                    down,
                    left,
                    right,
                    x,
                    y,
                    radius,
                } => {
                    unit(&format!("binding {n}: x"), *x)?;
                    unit(&format!("binding {n}: y"), *y)?;
                    if !(radius.is_finite() && *radius > 0.0 && *radius <= 0.5) {
                        return bad(format!(
                            "binding {n}: radius must be above 0 and at most 0.5"
                        ));
                    }
                    vec![up, down, left, right]
                }
                Binding::Look {
                    x,
                    y,
                    width,
                    height,
                    sensitivity,
                } => {
                    looks += 1;
                    unit(&format!("binding {n}: x"), *x)?;
                    unit(&format!("binding {n}: y"), *y)?;
                    let fits = |c: f64, size: f64| {
                        size.is_finite()
                            && size > 0.0
                            && c - size / 2.0 >= 0.0
                            && c + size / 2.0 <= 1.0
                    };
                    if !fits(*x, *width) || !fits(*y, *height) {
                        return bad(format!("binding {n}: the look area must lie on the screen"));
                    }
                    if !(sensitivity.is_finite() && *sensitivity > 0.0) {
                        return bad(format!("binding {n}: sensitivity must be above 0"));
                    }
                    vec![]
                }
            };
            for k in keys {
                if k.trim().is_empty() {
                    return bad(format!("binding {n}: a control name is empty"));
                }
                if used.contains(&k) {
                    return bad(format!("binding {n}: `{k}` is already bound"));
                }
                used.push(k);
            }
        }
        if looks > 1 {
            return bad("more than one look binding; the pointer can drive only one".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
        schema = 1
        package = "com.example.shooter"
        name = "Example Shooter"

        [[bind]]
        type = "tap"
        key = "Space"
        x = 0.9
        y = 0.8

        [[bind]]
        type = "joystick"
        up = "W"
        left = "A"
        down = "S"
        right = "D"
        x = 0.15
        y = 0.75
        radius = 0.08

        [[bind]]
        type = "look"
        x = 0.7
        y = 0.45
        width = 0.5
        height = 0.7
    "#;

    #[test]
    fn reads_a_profile() {
        let p = Profile::parse(GOOD).unwrap();
        assert_eq!(p.package, "com.example.shooter");
        assert_eq!(p.bindings.len(), 3);
        assert_eq!(
            p.bindings[0],
            Binding::Tap {
                key: "Space".into(),
                x: 0.9,
                y: 0.8
            }
        );
        match &p.bindings[2] {
            Binding::Look { sensitivity, .. } => assert_eq!(*sensitivity, 1.0, "the default"),
            other => panic!("{other:?}"),
        }
    }

    fn invalid(text: &str, want: &str) {
        match Profile::parse(text) {
            Err(e) => assert!(e.to_string().contains(want), "{e}"),
            Ok(_) => panic!("accepted: {text}"),
        }
    }

    #[test]
    fn refuses_what_cannot_work() {
        let head = "schema = 1\npackage = \"a.b\"\n";
        invalid("schema = 2\npackage = \"a.b\"", "schema 2");
        invalid("schema = 1\npackage = \" \"", "package is empty");
        invalid(
            &format!("{head}[[bind]]\ntype=\"tap\"\nkey=\"A\"\nx=1.2\ny=0.5"),
            "0.0 to 1.0",
        );
        invalid(
            &format!("{head}[[bind]]\ntype=\"tap\"\nkey=\"A\"\nx=0.1\ny=0.5\n[[bind]]\ntype=\"tap\"\nkey=\"A\"\nx=0.2\ny=0.5"),
            "already bound",
        );
        invalid(
            &format!("{head}[[bind]]\ntype=\"joystick\"\nup=\"W\"\ndown=\"S\"\nleft=\"A\"\nright=\"D\"\nx=0.1\ny=0.5\nradius=0.0"),
            "radius",
        );
        invalid(
            &format!("{head}[[bind]]\ntype=\"look\"\nx=0.9\ny=0.5\nwidth=0.5\nheight=0.5"),
            "must lie on the screen",
        );
        let look = "[[bind]]\ntype=\"look\"\nx=0.5\ny=0.5\nwidth=0.5\nheight=0.5\n";
        invalid(&format!("{head}{look}{look}"), "more than one look");
        let taps: String = (0..11)
            .map(|i| format!("[[bind]]\ntype=\"tap\"\nkey=\"K{i}\"\nx=0.5\ny=0.5\n"))
            .collect();
        invalid(&format!("{head}{taps}"), "10 fingers");
    }

    #[test]
    fn refuses_what_does_not_parse() {
        let syntax = |text: &str| matches!(Profile::parse(text), Err(ProfileError::Syntax(_)));
        assert!(syntax("schema = "));
        assert!(syntax("package = \"a.b\""), "schema is required");
        assert!(
            syntax("schema = 1\npackage = \"a.b\"\ncolour = \"red\""),
            "unknown field"
        );
        assert!(syntax(
            "schema = 1\npackage = \"a.b\"\n[[bind]]\ntype = \"wiggle\"\nkey = \"A\""
        ));
        assert!(syntax(
            "schema = 1\npackage = \"a.b\"\n[[bind]]\ntype = \"tap\"\nkey = \"A\"\nx = 0.5"
        ));
    }
}
