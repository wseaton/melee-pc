use std::ffi::c_int;

use crate::events::{PlayerSnapshot, Snapshot};

pub const PLAYER_SLOTS: usize = 6;
pub const PAD_PORTS: usize = 4;
pub const DEFAULT_SIM_HZ: u32 = 60;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

unsafe extern "C" {
    fn Player_GetPlayerSlotType(slot: i32) -> c_int;
    fn pc_debug_fighter_alive(slot: c_int) -> bool;
    fn pc_debug_pad(port: c_int, out: *mut RawPad);
    fn Player_GetPlayerCharacter(slot: c_int) -> c_int;
    fn Player_GetStocks(slot: c_int) -> i32;
    fn Player_SetStocks(slot: c_int, stocks: c_int);
    fn Player_GetDamage(slot: i32) -> i32;
    fn Player_LoadPlayerCoords(slot: i32, out: *mut Vec3);
    fn Player_GetKOsByPlayerIndex(slot: c_int, idx: c_int) -> i32;
    fn gm_GetCurrentSceneIndex() -> u8;
    fn gm_GetCurrentGameMode() -> u8;
    fn pc_get_sim_hz() -> u32;
    fn pc_set_sim_hz(hz: u32);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawPad {
    buttons: u32,
    stick_x: f32,
    stick_y: f32,
    cstick_x: f32,
    cstick_y: f32,
    trigger_l: f32,
    trigger_r: f32,
    connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadButton {
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

impl PadButton {
    pub fn mask(self) -> u32 {
        match self {
            Self::Left => 1 << 0,
            Self::Right => 1 << 1,
            Self::Down => 1 << 2,
            Self::Up => 1 << 3,
            Self::Z => 1 << 4,
            Self::R => 1 << 5,
            Self::L => 1 << 6,
            Self::A => 1 << 8,
            Self::B => 1 << 9,
            Self::X => 1 << 10,
            Self::Y => 1 << 11,
            Self::Start => 1 << 12,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadView {
    pub buttons: u32,
    pub stick: egui::Vec2,
    pub cstick: egui::Vec2,
    pub trigger_l: f32,
    pub trigger_r: f32,
}

impl PadView {
    pub fn pressed(&self, button: PadButton) -> bool {
        self.buttons & button.mask() != 0
    }
}

pub fn pads() -> [Option<PadView>; PAD_PORTS] {
    std::array::from_fn(|port| {
        let mut raw = RawPad::default();
        unsafe { pc_debug_pad(port as c_int, &raw mut raw) };
        raw.connected.then_some(PadView {
            buttons: raw.buttons,
            stick: egui::vec2(raw.stick_x, raw.stick_y),
            cstick: egui::vec2(raw.cstick_x, raw.cstick_y),
            trigger_l: raw.trigger_l,
            trigger_r: raw.trigger_r,
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(u8);

impl Slot {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..PLAYER_SLOTS as u8).map(Self)
    }

    pub fn number(self) -> u8 {
        self.0 + 1
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    fn raw(self) -> c_int {
        c_int::from(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerKind {
    Human,
    Cpu,
    Demo,
    Boss,
}

impl PlayerKind {
    fn from_raw(raw: c_int) -> Option<Self> {
        match raw {
            0 => Some(Self::Human),
            1 => Some(Self::Cpu),
            2 => Some(Self::Demo),
            4 => Some(Self::Boss),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "HMN",
            Self::Cpu => "CPU",
            Self::Demo => "DEMO",
            Self::Boss => "BOSS",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameMode(pub u8);

impl GameMode {
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 46] = [
            "title",
            "menu",
            "vs",
            "classic",
            "adventure",
            "allstar",
            "debug",
            "debug_sound_test",
            "hanyu_css",
            "hanyu_sss",
            "camera_mode",
            "toy_gallery",
            "toy_lottery",
            "toy_collection",
            "debug_vs",
            "target_test",
            "super_sudden_death_vs",
            "invisible_vs",
            "slomo_vs",
            "lightning_vs",
            "challenger_approach",
            "classic_gover",
            "adventure_gover",
            "allstar_gover",
            "opening_mv",
            "debug_cutscene",
            "debug_gover",
            "tournament",
            "training",
            "tiny_vs",
            "giant_vs",
            "stamina_vs",
            "home_run_contest",
            "10man_vs",
            "100man_vs",
            "3min_vs",
            "15min_vs",
            "endless_vs",
            "cruel_vs",
            "progressive_scan",
            "boot",
            "memcard",
            "camera_vs",
            "event",
            "single_button_vs",
            "online",
        ];
        NAMES.get(usize::from(self.0)).copied().unwrap_or("unknown")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Character(c_int);

impl Character {
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 33] = [
            "Captain Falcon",
            "Donkey Kong",
            "Fox",
            "Mr. Game & Watch",
            "Kirby",
            "Bowser",
            "Link",
            "Luigi",
            "Mario",
            "Marth",
            "Mewtwo",
            "Ness",
            "Peach",
            "Pikachu",
            "Ice Climbers",
            "Jigglypuff",
            "Samus",
            "Yoshi",
            "Zelda",
            "Sheik",
            "Falco",
            "Young Link",
            "Dr. Mario",
            "Roy",
            "Pichu",
            "Ganondorf",
            "Master Hand",
            "Male Wireframe",
            "Female Wireframe",
            "Giga Bowser",
            "Crazy Hand",
            "Sandbag",
            "Popo",
        ];
        usize::try_from(self.0)
            .ok()
            .and_then(|index| NAMES.get(index).copied())
            .unwrap_or("Unknown")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub slot: Slot,
    pub kind: PlayerKind,
    pub character: Character,
    pub stocks: i32,
    pub damage: i32,
    pub position: Vec3,
}

pub fn player(slot: Slot) -> Option<Player> {
    let kind = PlayerKind::from_raw(unsafe { Player_GetPlayerSlotType(slot.raw()) })?;
    if !unsafe { pc_debug_fighter_alive(slot.raw()) } {
        return None;
    }
    let mut position = Vec3::default();
    unsafe { Player_LoadPlayerCoords(slot.raw(), &raw mut position) };
    Some(Player {
        slot,
        kind,
        character: Character(unsafe { Player_GetPlayerCharacter(slot.raw()) }),
        stocks: unsafe { Player_GetStocks(slot.raw()) },
        damage: unsafe { Player_GetDamage(slot.raw()) },
        position,
    })
}

pub fn kos(killer: Slot) -> [i32; PLAYER_SLOTS] {
    let mut kos = [0; PLAYER_SLOTS];
    for victim in Slot::all() {
        kos[victim.index()] = unsafe { Player_GetKOsByPlayerIndex(killer.raw(), victim.raw()) };
    }
    kos
}

pub fn snapshot() -> Snapshot {
    let mut snapshot = Snapshot {
        mode: GameMode(unsafe { gm_GetCurrentGameMode() }),
        scene: scene_index(),
        ..Snapshot::default()
    };
    for slot in Slot::all() {
        snapshot.players[slot.index()] = player(slot).map(|p| PlayerSnapshot {
            kind: p.kind,
            character: p.character.name(),
            stocks: p.stocks,
            damage: p.damage,
            kos: kos(slot),
        });
    }
    snapshot
}

pub fn set_stocks(slot: Slot, stocks: i32) {
    unsafe { Player_SetStocks(slot.raw(), stocks) }
}

pub fn scene_index() -> u8 {
    unsafe { gm_GetCurrentSceneIndex() }
}

pub fn sim_hz() -> u32 {
    unsafe { pc_get_sim_hz() }
}

pub fn set_sim_hz(hz: u32) {
    unsafe { pc_set_sim_hz(hz) }
}

#[cfg(test)]
mod tests {
    use crate::game::{Character, GameMode, PadButton, PadView, PlayerKind, Slot};

    #[test]
    fn player_kind_maps_game_enum() {
        assert_eq!(PlayerKind::from_raw(0), Some(PlayerKind::Human));
        assert_eq!(PlayerKind::from_raw(1), Some(PlayerKind::Cpu));
        assert_eq!(PlayerKind::from_raw(2), Some(PlayerKind::Demo));
        assert_eq!(PlayerKind::from_raw(3), None);
        assert_eq!(PlayerKind::from_raw(4), Some(PlayerKind::Boss));
        assert_eq!(PlayerKind::from_raw(5), None);
        assert_eq!(PlayerKind::from_raw(-1), None);
    }

    #[test]
    fn character_names_match_ckind_order() {
        assert_eq!(Character(0x00).name(), "Captain Falcon");
        assert_eq!(Character(0x02).name(), "Fox");
        assert_eq!(Character(0x13).name(), "Sheik");
        assert_eq!(Character(0x19).name(), "Ganondorf");
        assert_eq!(Character(0x1A).name(), "Master Hand");
        assert_eq!(Character(0x20).name(), "Popo");
    }

    #[test]
    fn character_out_of_range_is_unknown() {
        assert_eq!(Character(0x21).name(), "Unknown");
        assert_eq!(Character(-1).name(), "Unknown");
    }

    #[test]
    fn game_mode_names_match_the_enum() {
        assert_eq!(GameMode(0x00).name(), "title");
        assert_eq!(GameMode(0x02).name(), "vs");
        assert_eq!(GameMode(0x0E).name(), "debug_vs");
        assert_eq!(GameMode(0x18).name(), "opening_mv");
        assert_eq!(GameMode(0x2D).name(), "online");
        assert_eq!(GameMode(0x2E).name(), "unknown");
        assert_eq!(GameMode(0xFF).name(), "unknown");
    }

    #[test]
    fn pad_button_masks_match_hsd_pad_bits() {
        let expected = [
            (PadButton::Left, 0x0001),
            (PadButton::Right, 0x0002),
            (PadButton::Down, 0x0004),
            (PadButton::Up, 0x0008),
            (PadButton::Z, 0x0010),
            (PadButton::R, 0x0020),
            (PadButton::L, 0x0040),
            (PadButton::A, 0x0100),
            (PadButton::B, 0x0200),
            (PadButton::X, 0x0400),
            (PadButton::Y, 0x0800),
            (PadButton::Start, 0x1000),
        ];
        for (button, mask) in expected {
            assert_eq!(button.mask(), mask, "{button:?}");
        }
    }

    #[test]
    fn pad_view_reads_pressed_buttons() {
        let pad = PadView {
            buttons: 0x1100,
            ..PadView::default()
        };
        assert!(pad.pressed(PadButton::A) && pad.pressed(PadButton::Start));
        assert!(!pad.pressed(PadButton::B) && !pad.pressed(PadButton::Z));
    }

    #[test]
    fn slots_cover_every_player() {
        let numbers: Vec<u8> = Slot::all().map(Slot::number).collect();
        assert_eq!(numbers, [1, 2, 3, 4, 5, 6]);
    }
}
