use std::fmt::{self, Write};

use crate::movie::{Frame, Movie};
use crate::pad::{Axis, Button, PORTS, Pad};

const DEFAULT_SEED: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub line: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorKind {
    BadNumber(String),
    BadPort(String),
    PortNotDeclared(u8),
    DuplicatePort(u8),
    UnknownToken(String),
    MissingValue(&'static str),
    BadAxis(String),
    ZeroRepeat,
    DirectiveAfterFrames(&'static str),
    EmptyClause,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: ", self.line)?;
        match &self.kind {
            ErrorKind::BadNumber(text) => write!(f, "'{text}' is not a valid number"),
            ErrorKind::BadPort(text) => {
                write!(f, "'{text}' is not a port, expected p1 to p{PORTS}")
            }
            ErrorKind::PortNotDeclared(port) => {
                write!(f, "p{port} is used but not listed in 'ports'")
            }
            ErrorKind::DuplicatePort(port) => write!(f, "p{port} appears twice"),
            ErrorKind::UnknownToken(text) => write!(f, "unknown input '{text}'"),
            ErrorKind::MissingValue(what) => write!(f, "'{what}' needs a value"),
            ErrorKind::BadAxis(text) => {
                write!(f, "'{text}' is not an axis, expected X,Y in -128..=127")
            }
            ErrorKind::ZeroRepeat => write!(f, "repeat count must be at least 1"),
            ErrorKind::DirectiveAfterFrames(name) => {
                write!(f, "'{name}' must come before the first frame")
            }
            ErrorKind::EmptyClause => write!(f, "port clause has no port"),
        }
    }
}

impl std::error::Error for ScriptError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Port(u8);

impl Port {
    fn parse(text: &str) -> Result<Self, ErrorKind> {
        let bad = || ErrorKind::BadPort(text.to_owned());
        let digits = text.strip_prefix(['p', 'P']).ok_or_else(bad)?;
        let number: u8 = digits.parse().map_err(|_| bad())?;
        Self::from_number(number).ok_or_else(bad)
    }

    fn from_number(number: u8) -> Option<Self> {
        (1..=PORTS as u8).contains(&number).then_some(Self(number))
    }

    fn index(self) -> usize {
        usize::from(self.0) - 1
    }
}

fn parse_u32(text: &str) -> Result<u32, ErrorKind> {
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| ErrorKind::BadNumber(text.to_owned()))
}

fn parse_axis(text: &str) -> Result<Axis, ErrorKind> {
    let bad = || ErrorKind::BadAxis(text.to_owned());
    let (x, y) = text.split_once(',').ok_or_else(bad)?;
    Ok(Axis {
        x: x.parse().map_err(|_| bad())?,
        y: y.parse().map_err(|_| bad())?,
    })
}

fn parse_clause(
    clause: &str,
    connected: &[bool; PORTS],
    frame: &mut Frame,
    used: &mut [bool; PORTS],
) -> Result<(), ErrorKind> {
    let mut tokens = clause.split_whitespace();
    let port = Port::parse(tokens.next().ok_or(ErrorKind::EmptyClause)?)?;
    if !connected[port.index()] {
        return Err(ErrorKind::PortNotDeclared(port.0));
    }
    if std::mem::replace(&mut used[port.index()], true) {
        return Err(ErrorKind::DuplicatePort(port.0));
    }
    let pad = &mut frame[port.index()];
    while let Some(token) = tokens.next() {
        let mut value = |what| tokens.next().ok_or(ErrorKind::MissingValue(what));
        match token.to_ascii_lowercase().as_str() {
            "stick" => pad.stick = parse_axis(value("stick")?)?,
            "cstick" => pad.cstick = parse_axis(value("cstick")?)?,
            "lt" => pad.left_trigger = parse_trigger(value("lt")?)?,
            "rt" => pad.right_trigger = parse_trigger(value("rt")?)?,
            _ => {
                let button = Button::from_name(token)
                    .ok_or_else(|| ErrorKind::UnknownToken(token.to_owned()))?;
                pad.buttons |= button.mask();
            }
        }
    }
    Ok(())
}

fn parse_trigger(text: &str) -> Result<u8, ErrorKind> {
    text.parse()
        .map_err(|_| ErrorKind::BadNumber(text.to_owned()))
}

fn split_repeat(line: &str) -> Result<(&str, usize), ErrorKind> {
    let Some((body, last)) = line.rsplit_once(char::is_whitespace) else {
        return Ok((line, 1));
    };
    let Some(count) = last
        .strip_prefix(['x', 'X'])
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    else {
        return Ok((line, 1));
    };
    match count.parse::<usize>() {
        Ok(0) => Err(ErrorKind::ZeroRepeat),
        Ok(count) => Ok((body, count)),
        Err(_) => Err(ErrorKind::BadNumber(last.to_owned())),
    }
}

pub fn parse(text: &str) -> Result<Movie, ScriptError> {
    let mut seed = DEFAULT_SEED;
    let mut connected = [true, false, false, false];
    let mut frames: Vec<Frame> = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let at = |kind| ScriptError {
            line: index + 1,
            kind,
        };
        let line = raw.split('#').next().unwrap_or_default().trim();
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        let rest = line[first.len()..].trim();
        let neutral: Frame = std::array::from_fn(|port| Pad::neutral(connected[port]));

        match first.to_ascii_lowercase().as_str() {
            "seed" | "ports" if !frames.is_empty() => {
                let name = if first.eq_ignore_ascii_case("seed") {
                    "seed"
                } else {
                    "ports"
                };
                return Err(at(ErrorKind::DirectiveAfterFrames(name)));
            }
            "seed" => seed = parse_u32(rest).map_err(at)?,
            "ports" => {
                connected = [false; PORTS];
                for token in rest.split_whitespace() {
                    let number: u8 = token
                        .parse()
                        .map_err(|_| at(ErrorKind::BadPort(token.to_owned())))?;
                    let port = Port::from_number(number)
                        .ok_or_else(|| at(ErrorKind::BadPort(token.to_owned())))?;
                    connected[port.index()] = true;
                }
            }
            "wait" => {
                let count = parse_u32(rest).map_err(at)?;
                frames.extend(std::iter::repeat_n(neutral, count as usize));
            }
            _ => {
                let (body, repeat) = split_repeat(line).map_err(at)?;
                let mut frame = neutral;
                let mut used = [false; PORTS];
                for clause in body.split('|') {
                    parse_clause(clause, &connected, &mut frame, &mut used).map_err(at)?;
                }
                frames.extend(std::iter::repeat_n(frame, repeat));
            }
        }
    }
    Ok(Movie { seed, frames })
}

fn format_pad(out: &mut String, port: usize, pad: &Pad) {
    let _ = write!(out, "p{}", port + 1);
    for button in Button::ALL {
        if pad.pressed(button) {
            let _ = write!(out, " {}", button.name());
        }
    }
    if pad.stick != Axis::default() {
        let _ = write!(out, " stick {}", pad.stick);
    }
    if pad.cstick != Axis::default() {
        let _ = write!(out, " cstick {}", pad.cstick);
    }
    if pad.left_trigger != 0 {
        let _ = write!(out, " lt {}", pad.left_trigger);
    }
    if pad.right_trigger != 0 {
        let _ = write!(out, " rt {}", pad.right_trigger);
    }
}

pub fn format(movie: &Movie) -> String {
    let connected: [bool; PORTS] = match movie.frames.first() {
        Some(frame) => std::array::from_fn(|port| frame[port].connected),
        None => [true, false, false, false],
    };
    let mut out = format!("seed {:#010x}\nports", movie.seed);
    for (port, _) in connected
        .iter()
        .enumerate()
        .filter(|(_, connected)| **connected)
    {
        let _ = write!(out, " {}", port + 1);
    }
    out.push('\n');

    for run in movie
        .frames
        .chunk_by(|a, b| inputs(a, &connected) == inputs(b, &connected))
    {
        let Some(frame) = run.first() else {
            continue;
        };
        let active: Vec<usize> = (0..PORTS)
            .filter(|&port| connected[port] && !frame[port].is_neutral())
            .collect();
        if active.is_empty() {
            let _ = writeln!(out, "wait {}", run.len());
            continue;
        }
        for (i, &port) in active.iter().enumerate() {
            if i > 0 {
                out.push_str(" | ");
            }
            format_pad(&mut out, port, &frame[port]);
        }
        if run.len() > 1 {
            let _ = write!(out, " x{}", run.len());
        }
        out.push('\n');
    }
    out
}

fn inputs(frame: &Frame, connected: &[bool; PORTS]) -> Frame {
    std::array::from_fn(|port| {
        if connected[port] {
            Pad {
                connected: true,
                ..frame[port]
            }
        } else {
            Pad::neutral(false)
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::movie::Movie;
    use crate::pad::{Axis, Button, Pad};
    use crate::script::{ErrorKind, ScriptError, format, parse};

    fn error(text: &str) -> ScriptError {
        parse(text).expect_err("script should be rejected")
    }

    #[test]
    fn empty_script_is_an_empty_movie_with_default_seed() {
        assert_eq!(
            parse(""),
            Ok(Movie {
                seed: 1,
                frames: Vec::new()
            })
        );
        assert_eq!(
            parse("\n  # only a comment\n\n"),
            Ok(Movie {
                seed: 1,
                frames: Vec::new()
            })
        );
    }

    #[test]
    fn seed_accepts_hex_and_decimal() {
        assert_eq!(parse("seed 0x2A").map(|m| m.seed), Ok(42));
        assert_eq!(parse("seed 42").map(|m| m.seed), Ok(42));
        assert_eq!(parse("SEED 0XFFFFFFFF").map(|m| m.seed), Ok(u32::MAX));
    }

    #[test]
    fn wait_emits_neutral_frames_with_only_port_one_connected() {
        let movie = parse("wait 3").expect("valid script");
        assert_eq!(movie.frames.len(), 3);
        for frame in &movie.frames {
            assert_eq!(frame[0], Pad::neutral(true));
            assert_eq!(frame[1..], [Pad::neutral(false); 3]);
        }
    }

    #[test]
    fn buttons_axes_and_triggers() {
        let movie =
            parse("p1 A start stick -128,127 cstick 5,-5 lt 255 rt 1").expect("valid script");
        assert_eq!(
            movie.frames,
            vec![[
                Pad {
                    buttons: Button::A.mask() | Button::Start.mask(),
                    stick: Axis { x: -128, y: 127 },
                    cstick: Axis { x: 5, y: -5 },
                    left_trigger: 255,
                    right_trigger: 1,
                    connected: true,
                },
                Pad::neutral(false),
                Pad::neutral(false),
                Pad::neutral(false),
            ]]
        );
    }

    #[test]
    fn repeat_suffix_holds_the_frame() {
        let movie = parse("p1 B x4\np1 A").expect("valid script");
        assert_eq!(movie.frames.len(), 5);
        assert!(
            movie.frames[..4]
                .iter()
                .all(|f| f[0].buttons == Button::B.mask())
        );
        assert_eq!(movie.frames[4][0].buttons, Button::A.mask());
    }

    #[test]
    fn x_button_is_not_mistaken_for_a_repeat() {
        let movie = parse("p1 A X").expect("valid script");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(
            movie.frames[0][0].buttons,
            Button::A.mask() | Button::X.mask()
        );
        assert_eq!(parse("p1 X x2").map(|m| m.frames.len()), Ok(2));
    }

    #[test]
    fn ports_directive_connects_pads_and_allows_clauses() {
        let movie = parse("ports 1 3\nwait 1\np1 A | p3 B x2").expect("valid script");
        assert_eq!(movie.frames.len(), 3);
        let connected: Vec<bool> = movie.frames[0].iter().map(|pad| pad.connected).collect();
        assert_eq!(connected, [true, false, true, false]);
        assert_eq!(movie.frames[2][0].buttons, Button::A.mask());
        assert_eq!(movie.frames[2][2].buttons, Button::B.mask());
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let movie = parse("# intro\n\nwait 2 # settle\n  p1 START   # go\n").expect("valid script");
        assert_eq!(movie.frames.len(), 3);
        assert_eq!(movie.frames[2][0].buttons, Button::Start.mask());
    }

    #[test]
    fn errors_carry_the_line_number() {
        assert_eq!(
            error("wait 1\n\np1 JUMP"),
            ScriptError {
                line: 3,
                kind: ErrorKind::UnknownToken("JUMP".into())
            }
        );
    }

    #[test]
    fn every_error_kind() {
        assert_eq!(
            error("seed banana").kind,
            ErrorKind::BadNumber("banana".into())
        );
        assert_eq!(error("wait -1").kind, ErrorKind::BadNumber("-1".into()));
        assert_eq!(error("p5 A").kind, ErrorKind::BadPort("p5".into()));
        assert_eq!(error("p0 A").kind, ErrorKind::BadPort("p0".into()));
        assert_eq!(error("ports 1 9").kind, ErrorKind::BadPort("9".into()));
        assert_eq!(error("q1 A").kind, ErrorKind::BadPort("q1".into()));
        assert_eq!(error("p2 A").kind, ErrorKind::PortNotDeclared(2));
        assert_eq!(error("p1 A | p1 B").kind, ErrorKind::DuplicatePort(1));
        assert_eq!(error("p1 stick").kind, ErrorKind::MissingValue("stick"));
        assert_eq!(error("p1 lt").kind, ErrorKind::MissingValue("lt"));
        assert_eq!(
            error("p1 stick 200,0").kind,
            ErrorKind::BadAxis("200,0".into())
        );
        assert_eq!(error("p1 stick 10").kind, ErrorKind::BadAxis("10".into()));
        assert_eq!(error("p1 rt 256").kind, ErrorKind::BadNumber("256".into()));
        assert_eq!(error("p1 A x0").kind, ErrorKind::ZeroRepeat);
        assert_eq!(error("p1 A |").kind, ErrorKind::EmptyClause);
        assert_eq!(
            error("wait 1\nseed 3").kind,
            ErrorKind::DirectiveAfterFrames("seed")
        );
        assert_eq!(
            error("p1 A\nports 1 2").kind,
            ErrorKind::DirectiveAfterFrames("ports")
        );
    }

    #[test]
    fn error_messages_name_the_line_and_problem() {
        assert_eq!(
            error("p2 A").to_string(),
            "line 1: p2 is used but not listed in 'ports'"
        );
        assert_eq!(
            error("\np1 stick 1;2").to_string(),
            "line 2: '1;2' is not an axis, expected X,Y in -128..=127"
        );
    }

    #[test]
    fn format_run_length_encodes() {
        let text = "seed 0x0000002a\nports 1 2\nwait 120\np1 START\nwait 30\np1 A stick 127,0 | p2 B x10\n";
        let movie = parse(text).expect("valid script");
        assert_eq!(movie.frames.len(), 161);
        assert_eq!(format(&movie), text);
    }

    #[test]
    fn format_of_an_empty_movie() {
        assert_eq!(
            format(&Movie {
                seed: 1,
                frames: Vec::new()
            }),
            "seed 0x00000001\nports 1\n"
        );
    }

    #[test]
    fn script_survives_the_binary_round_trip() {
        let text = "seed 0xdeadbeef\nports 1 4\nwait 5\np1 A B X Y Z L R START UP DOWN LEFT RIGHT x3\np4 cstick -128,127 lt 9 rt 8\n";
        let movie = parse(text).expect("valid script");
        let reread = Movie::from_bytes(&movie.to_bytes()).expect("valid movie");
        assert_eq!(reread, movie);
        assert_eq!(format(&reread), text);
    }

    #[test]
    fn format_ignores_inputs_on_disconnected_ports() {
        let mut movie = parse("wait 2").expect("valid script");
        movie.frames[1][2].buttons = Button::A.mask();
        assert_eq!(format(&movie), "seed 0x00000001\nports 1\nwait 2\n");
    }
}
