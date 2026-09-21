use serde::{Deserialize, Serialize};

const CENTIMETERS_PER_FOOT: f64 = 30.48;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Centimeters(pub i32);

impl Centimeters {
    pub fn from_feet(feet: f64) -> Self {
        Self((feet * CENTIMETERS_PER_FOOT).ceil() as i32)
    }

    pub fn feet(self) -> f64 {
        f64::from(self.0) / CENTIMETERS_PER_FOOT
    }

    pub fn meters(self) -> f64 {
        f64::from(self.0) / 100.0
    }

    pub fn feet_as_displayed(self) -> String {
        let tenths = (self.feet() * 10.0).floor() as i64;
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

#[cfg(test)]
mod tests {
    use crate::Centimeters;

    #[test]
    fn feet_are_truncated_the_way_the_game_shows_them() {
        assert_eq!(Centimeters(4720).feet_as_displayed(), "154.8");
        assert_eq!(Centimeters(0).feet_as_displayed(), "0.0");
        assert_eq!(Centimeters(3048).feet_as_displayed(), "100.0");
        assert_eq!(Centimeters(3047).feet_as_displayed(), "99.9");
    }

    #[test]
    fn a_length_in_feet_rounds_up_to_a_whole_centimeter() {
        assert_eq!(Centimeters::from_feet(100.0), Centimeters(3048));
        assert_eq!(Centimeters::from_feet(0.0), Centimeters(0));
        assert_eq!(Centimeters::from_feet(154.8), Centimeters(4719));
        assert_eq!(Centimeters::from_feet(0.01), Centimeters(1));
    }

    #[test]
    fn meters_and_feet_convert_from_centimeters() {
        assert!((Centimeters(4720).meters() - 47.2).abs() < 1e-9);
        assert!((Centimeters(3048).feet() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_wire_form_is_a_bare_number() {
        assert_eq!(serde_json::to_string(&Centimeters(4720)).unwrap(), "4720");
        let read: Centimeters = serde_json::from_str("4720").unwrap();
        assert_eq!(read, Centimeters(4720));
    }
}
