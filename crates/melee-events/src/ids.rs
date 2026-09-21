use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $name:ident, unknown = $unknown:literal {
            $($(#[$variant_meta:meta])* $variant:ident = $raw:literal => $wire:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant,)+
            Unknown,
        }

        impl $name {
            pub const KNOWN: &[Self] = &[$(Self::$variant,)+];

            pub fn from_raw(raw: i32) -> Self {
                match raw {
                    $($raw => Self::$variant,)+
                    _ => Self::Unknown,
                }
            }

            pub fn from_name(name: &str) -> Self {
                match name {
                    $($wire => Self::$variant,)+
                    _ => Self::Unknown,
                }
            }

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Unknown => $unknown,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer).map(|name| Self::from_name(&name))
            }
        }
    };
}

wire_enum! {
    PlayerKind, unknown = "?" {
        Human = 0 => "HMN",
        Cpu = 1 => "CPU",
        Demo = 2 => "DEMO",
        Boss = 4 => "BOSS",
    }
}

wire_enum! {
    #[derive(Default)]
    GameMode, unknown = "unknown" {
        #[default]
        Title = 0 => "title",
        Menu = 1 => "menu",
        Vs = 2 => "vs",
        Classic = 3 => "classic",
        Adventure = 4 => "adventure",
        AllStar = 5 => "allstar",
        Debug = 6 => "debug",
        DebugSoundTest = 7 => "debug_sound_test",
        HanyuCss = 8 => "hanyu_css",
        HanyuSss = 9 => "hanyu_sss",
        CameraMode = 10 => "camera_mode",
        ToyGallery = 11 => "toy_gallery",
        ToyLottery = 12 => "toy_lottery",
        ToyCollection = 13 => "toy_collection",
        DebugVs = 14 => "debug_vs",
        TargetTest = 15 => "target_test",
        SuperSuddenDeathVs = 16 => "super_sudden_death_vs",
        InvisibleVs = 17 => "invisible_vs",
        SlomoVs = 18 => "slomo_vs",
        LightningVs = 19 => "lightning_vs",
        ChallengerApproach = 20 => "challenger_approach",
        ClassicGover = 21 => "classic_gover",
        AdventureGover = 22 => "adventure_gover",
        AllStarGover = 23 => "allstar_gover",
        OpeningMv = 24 => "opening_mv",
        DebugCutscene = 25 => "debug_cutscene",
        DebugGover = 26 => "debug_gover",
        Tournament = 27 => "tournament",
        Training = 28 => "training",
        TinyVs = 29 => "tiny_vs",
        GiantVs = 30 => "giant_vs",
        StaminaVs = 31 => "stamina_vs",
        HomeRunContest = 32 => "home_run_contest",
        TenManVs = 33 => "10man_vs",
        HundredManVs = 34 => "100man_vs",
        ThreeMinVs = 35 => "3min_vs",
        FifteenMinVs = 36 => "15min_vs",
        EndlessVs = 37 => "endless_vs",
        CruelVs = 38 => "cruel_vs",
        ProgressiveScan = 39 => "progressive_scan",
        Boot = 40 => "boot",
        Memcard = 41 => "memcard",
        CameraVs = 42 => "camera_vs",
        Event = 43 => "event",
        SingleButtonVs = 44 => "single_button_vs",
        Online = 45 => "online",
    }
}

wire_enum! {
    Character, unknown = "Unknown" {
        CaptainFalcon = 0 => "Captain Falcon",
        DonkeyKong = 1 => "Donkey Kong",
        Fox = 2 => "Fox",
        GameAndWatch = 3 => "Mr. Game & Watch",
        Kirby = 4 => "Kirby",
        Bowser = 5 => "Bowser",
        Link = 6 => "Link",
        Luigi = 7 => "Luigi",
        Mario = 8 => "Mario",
        Marth = 9 => "Marth",
        Mewtwo = 10 => "Mewtwo",
        Ness = 11 => "Ness",
        Peach = 12 => "Peach",
        Pikachu = 13 => "Pikachu",
        IceClimbers = 14 => "Ice Climbers",
        Jigglypuff = 15 => "Jigglypuff",
        Samus = 16 => "Samus",
        Yoshi = 17 => "Yoshi",
        Zelda = 18 => "Zelda",
        Sheik = 19 => "Sheik",
        Falco = 20 => "Falco",
        YoungLink = 21 => "Young Link",
        DrMario = 22 => "Dr. Mario",
        Roy = 23 => "Roy",
        Pichu = 24 => "Pichu",
        Ganondorf = 25 => "Ganondorf",
        MasterHand = 26 => "Master Hand",
        MaleWireframe = 27 => "Male Wireframe",
        FemaleWireframe = 28 => "Female Wireframe",
        GigaBowser = 29 => "Giga Bowser",
        CrazyHand = 30 => "Crazy Hand",
        Sandbag = 31 => "Sandbag",
        Popo = 32 => "Popo",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{Character, GameMode, PlayerKind};

    #[test]
    fn raw_ids_match_the_game() {
        assert_eq!(GameMode::from_raw(0x0E), GameMode::DebugVs);
        assert_eq!(GameMode::from_raw(0x20), GameMode::HomeRunContest);
        assert_eq!(GameMode::from_raw(45), GameMode::Online);
        assert_eq!(Character::from_raw(15), Character::Jigglypuff);
        assert_eq!(Character::from_raw(31), Character::Sandbag);
        assert_eq!(PlayerKind::from_raw(4), PlayerKind::Boss);
    }

    #[test]
    fn player_kind_maps_game_enum() {
        assert_eq!(PlayerKind::from_raw(0), PlayerKind::Human);
        assert_eq!(PlayerKind::from_raw(1), PlayerKind::Cpu);
        assert_eq!(PlayerKind::from_raw(2), PlayerKind::Demo);
        assert_eq!(PlayerKind::from_raw(3), PlayerKind::Unknown);
        assert_eq!(PlayerKind::from_raw(4), PlayerKind::Boss);
        assert_eq!(PlayerKind::from_raw(5), PlayerKind::Unknown);
        assert_eq!(PlayerKind::from_raw(-1), PlayerKind::Unknown);
    }

    #[test]
    fn character_names_match_ckind_order() {
        assert_eq!(Character::from_raw(0x00).name(), "Captain Falcon");
        assert_eq!(Character::from_raw(0x02).name(), "Fox");
        assert_eq!(Character::from_raw(0x13).name(), "Sheik");
        assert_eq!(Character::from_raw(0x19).name(), "Ganondorf");
        assert_eq!(Character::from_raw(0x1A).name(), "Master Hand");
        assert_eq!(Character::from_raw(0x20).name(), "Popo");
        assert_eq!(Character::from_raw(0x21).name(), "Unknown");
        assert_eq!(Character::from_raw(-1).name(), "Unknown");
    }

    #[test]
    fn game_mode_names_match_the_enum() {
        assert_eq!(GameMode::from_raw(0x00).name(), "title");
        assert_eq!(GameMode::from_raw(0x02).name(), "vs");
        assert_eq!(GameMode::from_raw(0x0E).name(), "debug_vs");
        assert_eq!(GameMode::from_raw(0x18).name(), "opening_mv");
        assert_eq!(GameMode::from_raw(0x2D).name(), "online");
        assert_eq!(GameMode::from_raw(0x2E).name(), "unknown");
        assert_eq!(GameMode::from_raw(0xFF).name(), "unknown");
    }

    #[test]
    fn ids_outside_the_table_are_unknown() {
        assert_eq!(GameMode::from_raw(46), GameMode::Unknown);
        assert_eq!(GameMode::from_raw(-1), GameMode::Unknown);
        assert_eq!(Character::from_raw(33), Character::Unknown);
        assert_eq!(PlayerKind::from_raw(3), PlayerKind::Unknown);
    }

    #[test]
    fn known_ids_are_contiguous_where_the_game_says_so() {
        for (raw, mode) in GameMode::KNOWN.iter().enumerate() {
            assert_eq!(GameMode::from_raw(raw as i32), *mode);
        }
        for (raw, character) in Character::KNOWN.iter().enumerate() {
            assert_eq!(Character::from_raw(raw as i32), *character);
        }
        assert_eq!(GameMode::KNOWN.len(), 46);
        assert_eq!(Character::KNOWN.len(), 33);
    }

    #[test]
    fn wire_names_are_unique_and_round_trip() {
        let modes: HashSet<_> = GameMode::KNOWN.iter().map(|m| m.name()).collect();
        assert_eq!(modes.len(), GameMode::KNOWN.len());
        for mode in GameMode::KNOWN {
            assert_eq!(GameMode::from_name(mode.name()), *mode);
        }
        let characters: HashSet<_> = Character::KNOWN.iter().map(|c| c.name()).collect();
        assert_eq!(characters.len(), Character::KNOWN.len());
        for character in Character::KNOWN {
            assert_eq!(Character::from_name(character.name()), *character);
        }
        for kind in PlayerKind::KNOWN {
            assert_eq!(PlayerKind::from_name(kind.name()), *kind);
        }
    }

    #[test]
    fn a_name_from_a_newer_game_build_reads_as_unknown() {
        let mode: GameMode = serde_json::from_str(r#""break_the_targets_2""#).unwrap();
        assert_eq!(mode, GameMode::Unknown);
        assert_eq!(serde_json::to_string(&mode).unwrap(), r#""unknown""#);
    }

    #[test]
    fn a_non_string_is_an_error() {
        assert!(serde_json::from_str::<Character>("15").is_err());
    }

    #[test]
    fn the_default_mode_is_the_zeroed_one() {
        assert_eq!(GameMode::default(), GameMode::from_raw(0));
    }
}
