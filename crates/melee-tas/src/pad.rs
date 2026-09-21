use std::fmt;

pub const PAD_BYTES: usize = 16;
pub const PORTS: usize = 4;

const ERR_NONE: i8 = 0;
const ERR_NO_CONTROLLER: i8 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Down,
    Up,
    Z,
    R,
    L,
    A,
    B,
    X,
    Y,
    Start,
}

impl Button {
    pub const ALL: [Self; 12] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::Z,
        Self::L,
        Self::R,
        Self::Start,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];

    pub fn mask(self) -> u16 {
        match self {
            Self::Left => 0x0001,
            Self::Right => 0x0002,
            Self::Down => 0x0004,
            Self::Up => 0x0008,
            Self::Z => 0x0010,
            Self::R => 0x0020,
            Self::L => 0x0040,
            Self::A => 0x0100,
            Self::B => 0x0200,
            Self::X => 0x0400,
            Self::Y => 0x0800,
            Self::Start => 0x1000,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Down => "DOWN",
            Self::Up => "UP",
            Self::Z => "Z",
            Self::R => "R",
            Self::L => "L",
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Start => "START",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|button| button.name().eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Axis {
    pub x: i8,
    pub y: i8,
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.x, self.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u16,
    pub stick: Axis,
    pub cstick: Axis,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub connected: bool,
}

impl Pad {
    pub fn neutral(connected: bool) -> Self {
        Self {
            connected,
            ..Self::default()
        }
    }

    pub fn is_neutral(&self) -> bool {
        *self == Self::neutral(self.connected)
    }

    pub fn pressed(&self, button: Button) -> bool {
        self.buttons & button.mask() != 0
    }

    pub fn to_bytes(self) -> [u8; PAD_BYTES] {
        let mut bytes = [0; PAD_BYTES];
        bytes[0..2].copy_from_slice(&self.buttons.to_le_bytes());
        bytes[2] = self.stick.x.to_le_bytes()[0];
        bytes[3] = self.stick.y.to_le_bytes()[0];
        bytes[4] = self.cstick.x.to_le_bytes()[0];
        bytes[5] = self.cstick.y.to_le_bytes()[0];
        bytes[6] = self.left_trigger;
        bytes[7] = self.right_trigger;
        let err = if self.connected {
            ERR_NONE
        } else {
            ERR_NO_CONTROLLER
        };
        bytes[10] = err.to_le_bytes()[0];
        bytes
    }

    pub fn from_bytes(bytes: &[u8; PAD_BYTES]) -> Self {
        Self {
            buttons: u16::from_le_bytes([bytes[0], bytes[1]]),
            stick: Axis {
                x: i8::from_le_bytes([bytes[2]]),
                y: i8::from_le_bytes([bytes[3]]),
            },
            cstick: Axis {
                x: i8::from_le_bytes([bytes[4]]),
                y: i8::from_le_bytes([bytes[5]]),
            },
            left_trigger: bytes[6],
            right_trigger: bytes[7],
            connected: i8::from_le_bytes([bytes[10]]) == ERR_NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pad::{Axis, Button, Pad};

    #[test]
    fn masks_match_the_sdk_header() {
        let expected = [
            (Button::Left, 0x0001),
            (Button::Right, 0x0002),
            (Button::Down, 0x0004),
            (Button::Up, 0x0008),
            (Button::Z, 0x0010),
            (Button::R, 0x0020),
            (Button::L, 0x0040),
            (Button::A, 0x0100),
            (Button::B, 0x0200),
            (Button::X, 0x0400),
            (Button::Y, 0x0800),
            (Button::Start, 0x1000),
        ];
        for (button, mask) in expected {
            assert_eq!(button.mask(), mask, "{}", button.name());
        }
    }

    #[test]
    fn every_button_has_a_unique_mask_and_name() {
        for (i, a) in Button::ALL.iter().enumerate() {
            for b in &Button::ALL[i + 1..] {
                assert_ne!(a.mask(), b.mask());
                assert_ne!(a.name(), b.name());
            }
        }
    }

    #[test]
    fn names_parse_case_insensitively() {
        for button in Button::ALL {
            assert_eq!(Button::from_name(button.name()), Some(button));
            assert_eq!(
                Button::from_name(&button.name().to_lowercase()),
                Some(button)
            );
        }
        assert_eq!(Button::from_name("SELECT"), None);
        assert_eq!(Button::from_name(""), None);
    }

    #[test]
    fn byte_layout_matches_padstatus() {
        let pad = Pad {
            buttons: Button::A.mask() | Button::Start.mask(),
            stick: Axis { x: -128, y: 127 },
            cstick: Axis { x: 1, y: -1 },
            left_trigger: 200,
            right_trigger: 7,
            connected: true,
        };
        assert_eq!(
            pad.to_bytes(),
            [
                0x00, 0x11, 0x80, 0x7F, 0x01, 0xFF, 200, 7, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn disconnected_pad_reports_no_controller() {
        let bytes = Pad::neutral(false).to_bytes();
        assert_eq!(bytes[10], 0xFF);
        assert!(!Pad::from_bytes(&bytes).connected);
    }

    #[test]
    fn other_error_codes_read_as_disconnected() {
        let mut bytes = Pad::neutral(true).to_bytes();
        bytes[10] = 0xFE;
        assert!(!Pad::from_bytes(&bytes).connected);
    }

    #[test]
    fn bytes_round_trip() {
        let pad = Pad {
            buttons: 0x1F7F,
            stick: Axis { x: 55, y: -90 },
            cstick: Axis { x: -128, y: 127 },
            left_trigger: 255,
            right_trigger: 1,
            connected: true,
        };
        assert_eq!(Pad::from_bytes(&pad.to_bytes()), pad);
    }

    #[test]
    fn neutral_detection_ignores_connection_state() {
        assert!(Pad::neutral(true).is_neutral());
        assert!(Pad::neutral(false).is_neutral());
        let held = Pad {
            buttons: Button::B.mask(),
            ..Pad::neutral(true)
        };
        assert!(!held.is_neutral());
        assert!(held.pressed(Button::B) && !held.pressed(Button::A));
    }
}
