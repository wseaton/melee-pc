use std::fmt;

use crate::pad::{PAD_BYTES, PORTS, Pad};

const MAGIC: &[u8; 4] = b"MRC2";
const HEADER_BYTES: usize = 8;
const FRAME_BYTES: usize = PORTS * PAD_BYTES + 8;

pub type Frame = [Pad; PORTS];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Movie {
    pub seed: u32,
    pub frames: Vec<Frame>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MovieError {
    TooShort,
    BadMagic([u8; 4]),
    TrailingBytes(usize),
}

impl fmt::Display for MovieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "file is shorter than the {HEADER_BYTES}-byte header"),
            Self::BadMagic(magic) => write!(f, "expected magic MRC2, found {magic:02x?}"),
            Self::TrailingBytes(extra) => {
                write!(
                    f,
                    "{extra} trailing bytes do not form a whole {FRAME_BYTES}-byte frame"
                )
            }
        }
    }
}

impl std::error::Error for MovieError {}

pub fn next_seed(seed: u32) -> u32 {
    seed.wrapping_mul(214013).wrapping_add(2531011)
}

impl Movie {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_BYTES + self.frames.len() * FRAME_BYTES);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.seed.to_le_bytes());
        let mut seed = self.seed;
        for frame in &self.frames {
            for pad in frame {
                bytes.extend_from_slice(&pad.to_bytes());
            }
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&seed.to_le_bytes());
            seed = next_seed(seed);
        }
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MovieError> {
        let Some((header, body)) = bytes.split_first_chunk::<HEADER_BYTES>() else {
            return Err(MovieError::TooShort);
        };
        let [m0, m1, m2, m3, s0, s1, s2, s3] = *header;
        if [m0, m1, m2, m3] != *MAGIC {
            return Err(MovieError::BadMagic([m0, m1, m2, m3]));
        }
        let (records, trailing) = body.as_chunks::<FRAME_BYTES>();
        if !trailing.is_empty() {
            return Err(MovieError::TrailingBytes(trailing.len()));
        }
        let frames = records
            .iter()
            .map(|record| {
                let (pads, _) = record.as_chunks::<PAD_BYTES>();
                std::array::from_fn(|port| pads.get(port).map(Pad::from_bytes).unwrap_or_default())
            })
            .collect();
        Ok(Self {
            seed: u32::from_le_bytes([s0, s1, s2, s3]),
            frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::movie::{FRAME_BYTES, Movie, MovieError, next_seed};
    use crate::pad::{Axis, Button, Pad};

    fn sample() -> Movie {
        let p1 = Pad {
            buttons: Button::A.mask(),
            stick: Axis { x: 100, y: -100 },
            ..Pad::neutral(true)
        };
        Movie {
            seed: 0xDEAD_BEEF,
            frames: vec![
                [
                    Pad::neutral(true),
                    Pad::neutral(false),
                    Pad::neutral(false),
                    Pad::neutral(false),
                ],
                [
                    p1,
                    Pad::neutral(true),
                    Pad::neutral(false),
                    Pad::neutral(false),
                ],
            ],
        }
    }

    #[test]
    fn frame_record_is_72_bytes() {
        assert_eq!(FRAME_BYTES, 72);
    }

    #[test]
    fn header_and_size() {
        let bytes = sample().to_bytes();
        assert_eq!(&bytes[0..4], b"MRC2");
        assert_eq!(&bytes[4..8], &0xDEAD_BEEFu32.to_le_bytes());
        assert_eq!(bytes.len(), 8 + 2 * 72);
    }

    #[test]
    fn frame_seeds_follow_the_game_lcg() {
        let bytes = sample().to_bytes();
        let seed_at = |frame: usize| {
            let at = 8 + frame * 72 + 68;
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        assert_eq!(seed_at(0), 0xDEAD_BEEF);
        assert_eq!(seed_at(1), next_seed(0xDEAD_BEEF));
    }

    #[test]
    fn lcg_matches_hsd_rand() {
        assert_eq!(next_seed(0), 2531011);
        assert_eq!(next_seed(1), 214013 + 2531011);
        assert_eq!(next_seed(u32::MAX), 2531011u32.wrapping_sub(214013));
    }

    #[test]
    fn checksums_are_written_as_zero() {
        let bytes = sample().to_bytes();
        assert_eq!(&bytes[8 + 64..8 + 68], &[0, 0, 0, 0]);
    }

    #[test]
    fn round_trip() {
        let movie = sample();
        assert_eq!(Movie::from_bytes(&movie.to_bytes()), Ok(movie));
    }

    #[test]
    fn empty_movie_round_trips() {
        let movie = Movie {
            seed: 7,
            frames: Vec::new(),
        };
        assert_eq!(movie.to_bytes().len(), 8);
        assert_eq!(Movie::from_bytes(&movie.to_bytes()), Ok(movie));
    }

    #[test]
    fn rejects_short_files() {
        assert_eq!(Movie::from_bytes(b""), Err(MovieError::TooShort));
        assert_eq!(Movie::from_bytes(b"MRC2\0\0\0"), Err(MovieError::TooShort));
    }

    #[test]
    fn rejects_other_magic() {
        assert_eq!(
            Movie::from_bytes(b"MRC1\0\0\0\0"),
            Err(MovieError::BadMagic(*b"MRC1"))
        );
    }

    #[test]
    fn rejects_partial_frames() {
        let mut bytes = sample().to_bytes();
        bytes.truncate(bytes.len() - 5);
        assert_eq!(
            Movie::from_bytes(&bytes),
            Err(MovieError::TrailingBytes(67))
        );
    }

    #[test]
    fn reads_recordings_with_real_checksums_and_seeds() {
        let mut bytes = sample().to_bytes();
        bytes[8 + 64..8 + 72].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        assert_eq!(Movie::from_bytes(&bytes), Ok(sample()));
    }
}
