use melee_events::{Centimeters, Character, Event, PlayerKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub batter: Character,
    pub distance: Centimeters,
}

#[derive(Debug, Default)]
pub struct Run {
    batter: Option<Character>,
}

impl Run {
    pub fn observe(&mut self, event: &Event) -> Option<RunResult> {
        match event {
            Event::MatchStart { players } => {
                self.batter = players
                    .iter()
                    .find(|player| player.kind == PlayerKind::Human)
                    .map(|player| player.character);
                None
            }
            Event::HomeRunResult { distance } => Some(RunResult {
                batter: self.batter.unwrap_or(Character::Unknown),
                distance: *distance,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use melee_events::{Centimeters, Character, Event, GameMode, PlayerInfo, PlayerKind};

    use crate::run::{Run, RunResult};

    fn roster() -> Event {
        Event::MatchStart {
            players: vec![
                PlayerInfo {
                    player: 2,
                    kind: PlayerKind::Cpu,
                    character: Character::Sandbag,
                    stocks: 1,
                },
                PlayerInfo {
                    player: 1,
                    kind: PlayerKind::Human,
                    character: Character::Jigglypuff,
                    stocks: 1,
                },
            ],
        }
    }

    #[test]
    fn the_result_names_the_human_batter() {
        let mut run = Run::default();
        assert_eq!(run.observe(&roster()), None);
        assert_eq!(
            run.observe(&Event::HomeRunResult {
                distance: Centimeters(4720)
            }),
            Some(RunResult {
                batter: Character::Jigglypuff,
                distance: Centimeters(4720)
            })
        );
    }

    #[test]
    fn nothing_else_is_a_result() {
        let mut run = Run::default();
        let noise = [
            Event::ModeChange {
                from: GameMode::Title,
                to: GameMode::HomeRunContest,
            },
            Event::SceneChange { from: 0, to: 1 },
            Event::Damage {
                player: 2,
                from: 0,
                to: 28,
            },
            Event::MatchEnd,
        ];
        for event in &noise {
            assert_eq!(run.observe(event), None, "{event:?}");
        }
    }

    #[test]
    fn a_result_with_no_roster_has_an_unknown_batter() {
        let result = Run::default().observe(&Event::HomeRunResult {
            distance: Centimeters(0),
        });
        assert_eq!(
            result,
            Some(RunResult {
                batter: Character::Unknown,
                distance: Centimeters(0)
            })
        );
    }

    #[test]
    fn a_retry_replaces_the_batter() {
        let mut run = Run::default();
        run.observe(&roster());
        run.observe(&Event::MatchStart {
            players: vec![PlayerInfo {
                player: 1,
                kind: PlayerKind::Human,
                character: Character::Ganondorf,
                stocks: 1,
            }],
        });
        let result = run.observe(&Event::HomeRunResult {
            distance: Centimeters(100),
        });
        assert_eq!(result.map(|r| r.batter), Some(Character::Ganondorf));
    }
}
