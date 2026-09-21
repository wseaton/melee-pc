use melee_events::{Centimeters, Character, Event, GameMode, PlayerInfo, PlayerKind};

use crate::game::PLAYER_SLOTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerSnapshot {
    pub kind: PlayerKind,
    pub character: Character,
    pub stocks: i32,
    pub damage: i32,
    pub kos: [i32; PLAYER_SLOTS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub mode: GameMode,
    pub scene: u8,
    pub home_run: Option<Centimeters>,
    pub players: [Option<PlayerSnapshot>; PLAYER_SLOTS],
}

impl Snapshot {
    fn in_match(&self) -> bool {
        self.players
            .iter()
            .flatten()
            .any(|player| player.kind != PlayerKind::Demo)
    }

    fn roster(&self) -> Vec<PlayerInfo> {
        self.players
            .iter()
            .enumerate()
            .filter_map(|(slot, player)| {
                player.map(|p| PlayerInfo {
                    player: number(slot),
                    kind: p.kind,
                    character: p.character,
                    stocks: p.stocks,
                })
            })
            .collect()
    }
}

fn number(slot: usize) -> u8 {
    slot as u8 + 1
}

#[derive(Default)]
pub struct Tracker {
    previous: Option<Snapshot>,
}

impl Tracker {
    pub fn update(&mut self, now: Snapshot) -> Vec<Event> {
        let mut events = Vec::new();
        let before = self.previous.replace(now).unwrap_or_default();

        if before.mode != now.mode {
            events.push(Event::ModeChange {
                from: before.mode,
                to: now.mode,
            });
        }
        if before.scene != now.scene {
            events.push(Event::SceneChange {
                from: before.scene,
                to: now.scene,
            });
        }
        match (before.in_match(), now.in_match()) {
            (false, true) => events.push(Event::MatchStart {
                players: now.roster(),
            }),
            (true, false) => {
                if let Some(distance) = before.home_run {
                    events.push(Event::HomeRunResult { distance });
                }
                events.push(Event::MatchEnd);
            }
            _ => {}
        }
        for (slot, pair) in before.players.iter().zip(&now.players).enumerate() {
            let (Some(was), Some(is)) = pair else {
                continue;
            };
            if was.damage != is.damage {
                events.push(Event::Damage {
                    player: number(slot),
                    from: was.damage,
                    to: is.damage,
                });
            }
            if is.stocks < was.stocks {
                events.push(Event::StockLost {
                    player: number(slot),
                    stocks: is.stocks,
                });
            }
            for (victim, (kos_before, kos_now)) in was.kos.iter().zip(&is.kos).enumerate() {
                for _ in *kos_before..*kos_now {
                    events.push(if victim == slot {
                        Event::SelfDestruct {
                            player: number(slot),
                        }
                    } else {
                        Event::Ko {
                            killer: number(slot),
                            victim: number(victim),
                        }
                    });
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use melee_events::{Centimeters, Character, Envelope, Event, GameMode, PlayerInfo, PlayerKind};

    use crate::events::{PlayerSnapshot, Snapshot, Tracker};

    fn fighter(character: Character) -> PlayerSnapshot {
        PlayerSnapshot {
            kind: PlayerKind::Human,
            character,
            stocks: 4,
            damage: 0,
            kos: [0; 6],
        }
    }

    fn duel() -> Snapshot {
        let mut snapshot = Snapshot {
            scene: 2,
            ..Snapshot::default()
        };
        snapshot.players[0] = Some(fighter(Character::Fox));
        snapshot.players[1] = Some(fighter(Character::Marth));
        snapshot
    }

    fn settled(snapshot: Snapshot) -> Tracker {
        let mut tracker = Tracker::default();
        tracker.update(snapshot);
        tracker
    }

    #[test]
    fn first_sample_in_a_menu_is_silent() {
        assert_eq!(Tracker::default().update(Snapshot::default()), []);
    }

    #[test]
    fn scene_change_and_match_start_with_roster() {
        let mut tracker = settled(Snapshot {
            scene: 1,
            ..Snapshot::default()
        });
        assert_eq!(
            tracker.update(duel()),
            [
                Event::SceneChange { from: 1, to: 2 },
                Event::MatchStart {
                    players: vec![
                        PlayerInfo {
                            player: 1,
                            kind: PlayerKind::Human,
                            character: Character::Fox,
                            stocks: 4
                        },
                        PlayerInfo {
                            player: 2,
                            kind: PlayerKind::Human,
                            character: Character::Marth,
                            stocks: 4
                        },
                    ]
                },
            ]
        );
    }

    #[test]
    fn mode_change_is_reported_by_name_before_the_scene_change() {
        let mut tracker = settled(Snapshot::default());
        let next = Snapshot {
            mode: GameMode::DebugVs,
            scene: 1,
            ..Snapshot::default()
        };
        assert_eq!(
            tracker.update(next),
            [
                Event::ModeChange {
                    from: GameMode::Title,
                    to: GameMode::DebugVs
                },
                Event::SceneChange { from: 0, to: 1 }
            ]
        );
    }

    #[test]
    fn unchanged_state_emits_nothing() {
        let mut tracker = settled(duel());
        assert_eq!(tracker.update(duel()), []);
    }

    #[test]
    fn damage_up_and_down() {
        let mut tracker = settled(duel());
        let mut hit = duel();
        hit.players[1] = Some(PlayerSnapshot {
            damage: 13,
            ..fighter(Character::Marth)
        });
        assert_eq!(
            tracker.update(hit),
            [Event::Damage {
                player: 2,
                from: 0,
                to: 13
            }]
        );
        assert_eq!(
            tracker.update(duel()),
            [Event::Damage {
                player: 2,
                from: 13,
                to: 0
            }]
        );
    }

    #[test]
    fn ko_is_attributed_to_the_killer() {
        let mut tracker = settled(duel());
        let mut after = duel();
        let mut kos = [0; 6];
        kos[1] = 1;
        after.players[0] = Some(PlayerSnapshot {
            kos,
            ..fighter(Character::Fox)
        });
        after.players[1] = Some(PlayerSnapshot {
            stocks: 3,
            ..fighter(Character::Marth)
        });
        assert_eq!(
            tracker.update(after),
            [
                Event::Ko {
                    killer: 1,
                    victim: 2
                },
                Event::StockLost {
                    player: 2,
                    stocks: 3
                }
            ]
        );
    }

    #[test]
    fn own_slot_in_the_ko_table_is_a_self_destruct() {
        let mut tracker = settled(duel());
        let mut after = duel();
        let mut kos = [0; 6];
        kos[1] = 1;
        after.players[1] = Some(PlayerSnapshot {
            stocks: 3,
            kos,
            ..fighter(Character::Marth)
        });
        assert_eq!(
            tracker.update(after),
            [
                Event::StockLost {
                    player: 2,
                    stocks: 3
                },
                Event::SelfDestruct { player: 2 }
            ]
        );
    }

    #[test]
    fn several_kos_in_one_sample_are_all_reported() {
        let mut tracker = settled(duel());
        let mut after = duel();
        let mut kos = [0; 6];
        kos[1] = 2;
        after.players[0] = Some(PlayerSnapshot {
            kos,
            ..fighter(Character::Fox)
        });
        assert_eq!(
            tracker.update(after),
            [
                Event::Ko {
                    killer: 1,
                    victim: 2
                },
                Event::Ko {
                    killer: 1,
                    victim: 2
                }
            ]
        );
    }

    #[test]
    fn gaining_a_stock_is_not_a_loss() {
        let mut tracker = settled(duel());
        let mut after = duel();
        after.players[0] = Some(PlayerSnapshot {
            stocks: 5,
            ..fighter(Character::Fox)
        });
        assert_eq!(tracker.update(after), []);
    }

    #[test]
    fn match_end_when_every_player_is_gone() {
        let mut tracker = settled(duel());
        assert_eq!(
            tracker.update(Snapshot {
                scene: 1,
                ..Snapshot::default()
            }),
            [Event::SceneChange { from: 2, to: 1 }, Event::MatchEnd]
        );
    }

    #[test]
    fn results_screen_demo_fighters_are_not_a_match() {
        let mut tracker = settled(duel());
        let mut results = Snapshot {
            scene: 3,
            ..Snapshot::default()
        };
        results.players[0] = Some(PlayerSnapshot {
            kind: PlayerKind::Demo,
            ..fighter(Character::Fox)
        });
        assert_eq!(
            tracker.update(results),
            [Event::SceneChange { from: 2, to: 3 }, Event::MatchEnd]
        );
        assert_eq!(tracker.update(results), []);
    }

    fn home_run_contest(distance: i32) -> Snapshot {
        let mut snapshot = Snapshot {
            mode: GameMode::HomeRunContest,
            scene: 1,
            home_run: Some(Centimeters(distance)),
            ..Snapshot::default()
        };
        snapshot.players[0] = Some(fighter(Character::Jigglypuff));
        snapshot.players[1] = Some(PlayerSnapshot {
            kind: PlayerKind::Cpu,
            ..fighter(Character::Sandbag)
        });
        snapshot
    }

    #[test]
    fn a_home_run_contest_reports_its_distance_before_the_match_end() {
        let mut tracker = settled(home_run_contest(0));
        assert_eq!(tracker.update(home_run_contest(1200)), []);
        assert_eq!(tracker.update(home_run_contest(4720)), []);
        let left = Snapshot {
            mode: GameMode::HomeRunContest,
            scene: 1,
            home_run: Some(Centimeters(0)),
            ..Snapshot::default()
        };
        assert_eq!(
            tracker.update(left),
            [
                Event::HomeRunResult {
                    distance: Centimeters(4720)
                },
                Event::MatchEnd
            ]
        );
        assert_eq!(tracker.update(left), []);
    }

    #[test]
    fn a_bag_that_never_left_the_platform_is_a_zero_distance_result() {
        let mut tracker = settled(home_run_contest(0));
        let left = Snapshot {
            mode: GameMode::HomeRunContest,
            scene: 1,
            ..Snapshot::default()
        };
        assert_eq!(
            tracker.update(left),
            [
                Event::HomeRunResult {
                    distance: Centimeters(0)
                },
                Event::MatchEnd
            ]
        );
    }

    #[test]
    fn a_distance_changing_mid_flight_is_not_an_event() {
        let mut tracker = settled(home_run_contest(0));
        for distance in [10, 500, 4000, 4720] {
            assert_eq!(tracker.update(home_run_contest(distance)), []);
        }
    }

    #[test]
    fn a_match_outside_home_run_contest_has_no_result() {
        let mut tracker = settled(duel());
        let results = Snapshot {
            scene: 3,
            ..Snapshot::default()
        };
        let events = tracker.update(results);
        assert!(events.contains(&Event::MatchEnd));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::HomeRunResult { .. }))
        );
    }

    #[test]
    fn a_player_leaving_mid_match_is_not_a_match_end() {
        let mut tracker = settled(duel());
        let mut after = duel();
        after.players[1] = None;
        assert_eq!(tracker.update(after), []);
    }

    #[test]
    fn envelope_flattens_into_one_json_object() {
        let envelope = Envelope {
            seq: 7,
            frame: 1492,
            time_ms: 1_790_000_000_000,
            dropped: 0,
            event: Event::Ko {
                killer: 1,
                victim: 2,
            },
        };
        assert_eq!(
            serde_json::to_string(&envelope).expect("serializable"),
            r#"{"seq":7,"frame":1492,"time_ms":1790000000000,"dropped":0,"type":"ko","killer":1,"victim":2}"#
        );
    }

    #[test]
    fn unit_and_nested_events_serialize() {
        assert_eq!(
            serde_json::to_string(&Event::MatchEnd).expect("serializable"),
            r#"{"type":"match_end"}"#
        );
        let start = Event::MatchStart {
            players: vec![PlayerInfo {
                player: 3,
                kind: PlayerKind::Cpu,
                character: Character::Mario,
                stocks: 99,
            }],
        };
        assert_eq!(
            serde_json::to_string(&start).expect("serializable"),
            r#"{"type":"match_start","players":[{"player":3,"kind":"CPU","character":"Mario","stocks":99}]}"#
        );
    }
}
