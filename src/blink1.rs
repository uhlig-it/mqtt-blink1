use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum Command {
    Color { color: Color },
    Blink { blink: Blink },
}

#[derive(Deserialize, Serialize)]
pub struct Color {
    #[serde(default)]
    pub r: u8,
    #[serde(default)]
    pub g: u8,
    #[serde(default)]
    pub b: u8,
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Color(r: {}, g: {}, b: {})", self.r, self.g, self.b)
    }
}

// Defense in depth (§3.2): cap blink parameters so that a single message
// cannot monopolize the service in a USB-write busy loop — the blink loop
// runs synchronously in the consuming thread.
const MAX_BLINK_COUNT: u64 = 100;
const MIN_BLINK_INTERVAL_MS: u64 = 10;

#[derive(Deserialize, Serialize)]
pub struct Blink {
    #[serde(default)]
    pub interval_ms: u64,
    #[serde(default)]
    pub count: u64,
    pub color: Color,
}

impl Blink {
    /// Rejects parameters outside the caps (§3.2). The caller logs the
    /// returned error and ignores the message instead of executing it.
    pub fn validate(&self) -> Result<(), String> {
        if self.count > MAX_BLINK_COUNT {
            return Err(format!(
                "blink count {} exceeds the maximum of {MAX_BLINK_COUNT}",
                self.count
            ));
        }
        if self.interval_ms < MIN_BLINK_INTERVAL_MS {
            return Err(format!(
                "blink interval {}ms is below the minimum of {MIN_BLINK_INTERVAL_MS}ms",
                self.interval_ms
            ));
        }
        Ok(())
    }
}

impl fmt::Display for Blink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Blink(frequency: {}, count: {}, color: {})",
            self.interval_ms, self.count, self.color
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- §3.2: blink parameter caps (boundary values) ---

    #[test]
    fn test_blink_validate_accepts_boundary_values() {
        let blink = Blink {
            interval_ms: MIN_BLINK_INTERVAL_MS,
            count: MAX_BLINK_COUNT,
            color: Color { r: 255, g: 0, b: 0 },
        };

        assert!(blink.validate().is_ok());
    }

    #[test]
    fn test_blink_validate_rejects_count_above_max() {
        let blink = Blink {
            interval_ms: MIN_BLINK_INTERVAL_MS,
            count: MAX_BLINK_COUNT + 1,
            color: Color { r: 255, g: 0, b: 0 },
        };

        assert!(blink.validate().is_err());
    }

    #[test]
    fn test_blink_validate_rejects_interval_below_min() {
        let blink = Blink {
            interval_ms: MIN_BLINK_INTERVAL_MS - 1,
            count: 0,
            color: Color { r: 255, g: 0, b: 0 },
        };

        assert!(blink.validate().is_err());
    }

    #[test]
    fn test_blink_validate_accepts_default_count_zero() {
        // `count` defaults to 0 (a no-op blink, see `test_deserialize_blink`)
        // — the caps must leave that default alone.
        let blink = Blink {
            interval_ms: MIN_BLINK_INTERVAL_MS,
            count: 0,
            color: Color { r: 255, g: 0, b: 0 },
        };

        assert!(blink.validate().is_ok());
    }

    #[test]
    fn test_deserialize_color() {
        let str = r#"{"color":{"r":127,"g":12,"b":24}}"#;

        let result: serde_json::Result<Command> = serde_json::from_str(str);

        match result {
            Ok(cmd) => match cmd {
                Command::Blink { blink } => panic!("did not expect {}", blink),
                Command::Color { color } => {
                    assert_eq!(color.r, 127);
                    assert_eq!(color.g, 12);
                    assert_eq!(color.b, 24);
                }
            },
            Err(e) => {
                panic!("Unable to parse message: {}", e);
            }
        }
    }

    #[test]
    fn test_deserialize_blink() {
        let str = r#"{"blink":{"interval_ms":200,"color":{"r":13,"g":8,"b":247}}}"#;

        let result: serde_json::Result<Command> = serde_json::from_str(str);

        match result {
            Ok(cmd) => match cmd {
                Command::Blink { blink } => {
                    assert_eq!(blink.interval_ms, 200);
                    assert_eq!(blink.count, 0);
                    assert_eq!(blink.color.r, 13);
                    assert_eq!(blink.color.g, 8);
                    assert_eq!(blink.color.b, 247);
                }
                Command::Color { color } => panic!("did not expect {}", color),
            },
            Err(e) => {
                panic!("Unable to parse message: {}", e);
            }
        }
    }
}
