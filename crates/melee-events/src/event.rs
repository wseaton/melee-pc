use serde::{Deserialize, Serialize};

use crate::ids::{Character, GameMode, PlayerKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub player: u8,
    pub kind: PlayerKind,
    pub character: Character,
    pub stocks: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ModeChange { from: GameMode, to: GameMode },
    SceneChange { from: u8, to: u8 },
    MatchStart { players: Vec<PlayerInfo> },
    MatchEnd,
    Damage { player: u8, from: i32, to: i32 },
    StockLost { player: u8, stocks: i32 },
    Ko { killer: u8, victim: u8 },
    SelfDestruct { player: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub seq: u64,
    pub frame: u64,
    pub time_ms: u64,
    pub dropped: u64,
    #[serde(flatten)]
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use crate::{Character, Envelope, Event, GameMode, PlayerInfo, PlayerKind};

    fn every_event() -> Vec<Event> {
        vec![
            Event::ModeChange {
                from: GameMode::Title,
                to: GameMode::HomeRunContest,
            },
            Event::SceneChange { from: 0, to: 1 },
            Event::MatchStart {
                players: vec![
                    PlayerInfo {
                        player: 1,
                        kind: PlayerKind::Human,
                        character: Character::Jigglypuff,
                        stocks: 1,
                    },
                    PlayerInfo {
                        player: 2,
                        kind: PlayerKind::Cpu,
                        character: Character::Sandbag,
                        stocks: 1,
                    },
                ],
            },
            Event::MatchEnd,
            Event::Damage {
                player: 2,
                from: 0,
                to: 28,
            },
            Event::StockLost {
                player: 1,
                stocks: 3,
            },
            Event::Ko {
                killer: 1,
                victim: 2,
            },
            Event::SelfDestruct { player: 4 },
        ]
    }

    fn envelope(event: Event) -> Envelope {
        Envelope {
            seq: 7,
            frame: 1234,
            time_ms: 1_758_400_000_000,
            dropped: 2,
            event,
        }
    }

    #[test]
    fn every_event_survives_a_round_trip() {
        for event in every_event() {
            let sent = envelope(event);
            let line = serde_json::to_string(&sent).unwrap();
            let received: Envelope = serde_json::from_str(&line).unwrap();
            assert_eq!(received, sent, "{line}");
        }
    }

    #[test]
    fn the_wire_format_is_flat_and_tagged() {
        let line = serde_json::to_string(&envelope(Event::Ko {
            killer: 1,
            victim: 2,
        }))
        .unwrap();
        assert_eq!(
            line,
            r#"{"seq":7,"frame":1234,"time_ms":1758400000000,"dropped":2,"type":"ko","killer":1,"victim":2}"#
        );
    }

    #[test]
    fn a_roster_entry_keeps_its_wire_names() {
        let line = serde_json::to_string(&PlayerInfo {
            player: 2,
            kind: PlayerKind::Cpu,
            character: Character::Sandbag,
            stocks: 1,
        })
        .unwrap();
        assert_eq!(
            line,
            r#"{"player":2,"kind":"CPU","character":"Sandbag","stocks":1}"#
        );
    }

    #[test]
    fn a_mode_change_names_both_modes() {
        let line = serde_json::to_string(&Event::ModeChange {
            from: GameMode::Menu,
            to: GameMode::HomeRunContest,
        })
        .unwrap();
        assert_eq!(
            line,
            r#"{"type":"mode_change","from":"menu","to":"home_run_contest"}"#
        );
    }

    #[test]
    fn an_unknown_event_type_is_an_error() {
        let line = r#"{"seq":1,"frame":1,"time_ms":1,"dropped":0,"type":"teleport"}"#;
        assert!(serde_json::from_str::<Envelope>(line).is_err());
    }

    #[test]
    fn a_missing_field_is_an_error() {
        let line = r#"{"seq":1,"frame":1,"time_ms":1,"dropped":0,"type":"ko","killer":1}"#;
        assert!(serde_json::from_str::<Envelope>(line).is_err());
    }
}
