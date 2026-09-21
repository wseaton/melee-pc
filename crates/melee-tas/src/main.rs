use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

use melee_tas::movie::Movie;
use melee_tas::script;

const USAGE: &str = "usage:
  melee-tas compile <script.tas> <movie.mrc>
  melee-tas decompile <movie.mrc> <script.tas>

Play a movie with the port's own replay: MELEE_NET_REPLAY=<movie.mrc> melee <disc>";

enum Command {
    Compile,
    Decompile,
}

fn run(command: Command, input: PathBuf, output: PathBuf) -> Result<usize, Box<dyn Error>> {
    let movie = match command {
        Command::Compile => {
            let movie = script::parse(&fs::read_to_string(&input)?)?;
            fs::write(&output, movie.to_bytes())?;
            movie
        }
        Command::Decompile => {
            let movie = Movie::from_bytes(&fs::read(&input)?)?;
            fs::write(&output, script::format(&movie))?;
            movie
        }
    };
    Ok(movie.frames.len())
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = match args.next().as_deref() {
        Some("compile") => Command::Compile,
        Some("decompile") => Command::Decompile,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (Some(input), Some(output), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(command, input.into(), PathBuf::from(&output)) {
        Ok(frames) => {
            println!("{output}: {frames} frames");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("melee-tas: {error}");
            ExitCode::FAILURE
        }
    }
}
