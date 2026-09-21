use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::game::Player;

pub const HEADER: &str = "frame,player,character,x,y,damage";

pub fn row(frame: u64, player: &Player) -> String {
    format!(
        "{frame},{},{},{:.3},{:.3},{}",
        player.slot.number(),
        player.character.name(),
        player.position.x,
        player.position.y,
        player.damage
    )
}

pub struct Trace {
    out: BufWriter<File>,
}

impl Trace {
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        writeln!(out, "{HEADER}")?;
        Ok(Self { out })
    }

    pub fn record(&mut self, frame: u64, players: &[Player]) -> io::Result<()> {
        for player in players {
            writeln!(self.out, "{}", row(frame, player))?;
        }
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use melee_events::{Character, PlayerKind};

    use crate::game::{Player, Slot, Vec3};
    use crate::trace::{HEADER, Trace, row};

    fn player(slot: usize, character: Character, x: f32, damage: i32) -> Player {
        Player {
            slot: Slot::all().nth(slot).unwrap(),
            kind: PlayerKind::Human,
            character,
            stocks: 1,
            damage,
            position: Vec3 { x, y: 0.5, z: 0.0 },
        }
    }

    #[test]
    fn a_row_is_frame_player_character_position_damage() {
        assert_eq!(
            row(142, &player(1, Character::Sandbag, -3.25, 28)),
            "142,2,Sandbag,-3.250,0.500,28"
        );
    }

    #[test]
    fn a_trace_file_has_a_header_and_one_row_per_player_per_frame() {
        let path =
            std::env::temp_dir().join(format!("melee-trace-test-{}.csv", std::process::id()));
        let mut trace = Trace::create(&path).unwrap();
        let players = [
            player(0, Character::Jigglypuff, -20.0, 0),
            player(1, Character::Sandbag, 0.0, 0),
        ];
        trace.record(1, &players).unwrap();
        trace.record(2, &players[..1]).unwrap();
        trace.record(3, &[]).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(
            written.lines().collect::<Vec<_>>(),
            [
                HEADER,
                "1,1,Jigglypuff,-20.000,0.500,0",
                "1,2,Sandbag,0.000,0.500,0",
                "2,1,Jigglypuff,-20.000,0.500,0",
            ]
        );
    }

    #[test]
    fn an_unwritable_path_is_an_error() {
        assert!(Trace::create(std::path::Path::new("/nonexistent-dir/trace.csv")).is_err());
    }
}
