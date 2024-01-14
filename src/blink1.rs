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
    pub r: u64,
    #[serde(default)]
    pub g: u64,
    #[serde(default)]
    pub b: u64,
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Color(r: {}, g: {}, b: {})", self.r, self.g, self.b)
    }
}

#[derive(Deserialize, Serialize)]
pub struct Blink {
    #[serde(default)]
    pub frequency: f64,
    #[serde(default)]
    pub count: u64,
    pub color: Color,
}

impl fmt::Display for Blink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blink(frequency: {}, count: {}, color: {})", self.frequency, self.count, self.color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                },
            },
            Err(e) => {
                panic!("Unable to parse message: {}", e);
            }
        }
    }

    #[test]
    fn test_deserialize_blink() {
        let str = r#"{"blink":{"frequency":2,"color":{"r":13,"g":8,"b":247}}}"#;

        let result: serde_json::Result<Command> = serde_json::from_str(str);

        match result {
            Ok(cmd) => match cmd {
                Command::Blink { blink } => {
                  assert_eq!(blink.frequency, 2.0);
                  assert_eq!(blink.count, 0);
                  assert_eq!(blink.color.r, 13);
                  assert_eq!(blink.color.g, 8);
                  assert_eq!(blink.color.b, 247);
                },
                Command::Color { color } => panic!("did not expect {}", color),
            },
            Err(e) => {
                panic!("Unable to parse message: {}", e);
            }
        }
    }
}
