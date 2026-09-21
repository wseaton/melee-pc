use std::io::ErrorKind;

use melee_events::{Command, Envelope};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpListener;

use crate::error::Error;
use crate::run::{Run, RunResult};

async fn next_line<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>) -> Result<Option<String>, Error> {
    match lines.next_line().await {
        Ok(line) => Ok(line),
        Err(error) if error.kind() == ErrorKind::ConnectionReset => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub async fn await_result(listener: &TcpListener, greeting: &Command) -> Result<RunResult, Error> {
    let (stream, peer) = listener.accept().await?;
    println!("game connected from {peer}");
    let (reader, mut writer) = stream.into_split();

    let mut line = serde_json::to_string(greeting).map_err(std::io::Error::from)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;

    let mut run = Run::default();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = next_line(&mut lines).await? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Envelope>(&line) {
            Ok(envelope) => {
                if let Some(result) = run.observe(&envelope.event) {
                    return Ok(result);
                }
            }
            Err(error) => eprintln!("skipping a line this build cannot read ({error}): {line}"),
        }
    }
    Err(Error::StreamClosed)
}

#[cfg(test)]
mod tests {
    use melee_events::{Centimeters, Character, Command};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};

    use crate::error::Error;
    use crate::run::RunResult;
    use crate::stream::await_result;

    const RECORDED: &str = concat!(
        r#"{"seq":0,"frame":3,"time_ms":1789998103100,"dropped":0,"type":"mode_change","from":"title","to":"home_run_contest"}"#,
        "\n",
        r#"{"seq":1,"frame":3,"time_ms":1789998103100,"dropped":0,"type":"scene_change","from":0,"to":1}"#,
        "\n",
        r#"{"seq":2,"frame":3,"time_ms":1789998103100,"dropped":0,"type":"match_start","players":[{"player":1,"kind":"HMN","character":"Jigglypuff","stocks":1},{"player":2,"kind":"CPU","character":"Sandbag","stocks":1}]}"#,
        "\n",
        r#"{"seq":3,"frame":142,"time_ms":1789998105416,"dropped":0,"type":"damage","player":2,"from":0,"to":28}"#,
        "\n",
        "\n",
        r#"{"seq":4,"frame":500,"time_ms":1789998110000,"dropped":0,"type":"from_a_newer_build","x":1}"#,
        "\n",
        "not json at all\n",
        r#"{"seq":6,"frame":742,"time_ms":1789998115416,"dropped":0,"type":"home_run_result","distance":4720}"#,
        "\n",
        r#"{"seq":7,"frame":742,"time_ms":1789998115416,"dropped":0,"type":"match_end"}"#,
        "\n",
    );

    fn nameplate() -> Command {
        Command::Nameplate {
            key: "DEMO-7".to_owned(),
            summary: "Sandbag".to_owned(),
        }
    }

    #[tokio::test]
    async fn a_recorded_run_yields_its_result_and_the_game_gets_the_nameplate() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let game = tokio::spawn(async move {
            let stream = TcpStream::connect(addr).await.unwrap();
            let (reader, mut writer) = stream.into_split();
            writer.write_all(RECORDED.as_bytes()).await.unwrap();
            let mut greeting = String::new();
            BufReader::new(reader)
                .read_line(&mut greeting)
                .await
                .unwrap();
            greeting
        });

        let result = await_result(&listener, &nameplate()).await.unwrap();
        assert_eq!(
            result,
            RunResult {
                batter: Character::Jigglypuff,
                distance: Centimeters(4720)
            }
        );
        let greeting = game.await.unwrap();
        assert_eq!(
            serde_json::from_str::<Command>(greeting.trim_end()).unwrap(),
            nameplate()
        );
        assert!(greeting.ends_with('\n'));
    }

    #[tokio::test]
    async fn a_game_that_quits_before_a_result_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            let partial: String = RECORDED.lines().take(4).map(|l| format!("{l}\n")).collect();
            stream.write_all(partial.as_bytes()).await.unwrap();
        });
        assert!(matches!(
            await_result(&listener, &nameplate()).await,
            Err(Error::StreamClosed)
        ));
    }
}
