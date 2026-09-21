use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use melee_events::Command;

pub const QUEUE_CAPACITY: usize = 4096;
const INBOX_CAPACITY: usize = 64;
const FIRST_RETRY: Duration = Duration::from_millis(250);
const MAX_RETRY: Duration = Duration::from_secs(5);

pub struct Shipper {
    queue: SyncSender<String>,
    inbox: Receiver<Command>,
    dropped: Arc<AtomicU64>,
}

impl Shipper {
    pub fn spawn(addr: String) -> io::Result<Self> {
        let (queue, lines) = sync_channel(QUEUE_CAPACITY);
        let (commands, inbox) = sync_channel(INBOX_CAPACITY);
        thread::Builder::new()
            .name("event-shipper".into())
            .spawn(move || ship(&addr, &lines, &commands))?;
        Ok(Self {
            queue,
            inbox,
            dropped: Arc::default(),
        })
    }

    pub fn send(&self, line: String) {
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.queue.try_send(line)
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn commands(&self) -> impl Iterator<Item = Command> + '_ {
        self.inbox.try_iter()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

fn connect(addr: &str) -> TcpStream {
    let mut retry = FIRST_RETRY;
    let mut reported = false;
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                eprintln!("events: connected to {addr}");
                return stream;
            }
            Err(error) => {
                if !reported {
                    eprintln!("events: cannot reach {addr} ({error}), retrying");
                    reported = true;
                }
                thread::sleep(retry);
                retry = (retry * 2).min(MAX_RETRY);
            }
        }
    }
}

fn listen(stream: &TcpStream, commands: &SyncSender<Command>) {
    let reader = match stream.try_clone() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("events: cannot read commands ({error})");
            return;
        }
    };
    let commands = commands.clone();
    let spawned = thread::Builder::new()
        .name("event-commands".into())
        .spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else {
                    return;
                };
                match serde_json::from_str(&line) {
                    Ok(command) => {
                        if let Err(TrySendError::Disconnected(_)) = commands.try_send(command) {
                            return;
                        }
                    }
                    Err(error) => eprintln!("events: ignoring command ({error}): {line}"),
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("events: cannot start command thread: {error}");
    }
}

fn ship(addr: &str, lines: &Receiver<String>, commands: &SyncSender<Command>) {
    let mut stream = connect(addr);
    listen(&stream, commands);
    while let Ok(mut line) = lines.recv() {
        line.push('\n');
        while let Err(error) = stream.write_all(line.as_bytes()) {
            eprintln!("events: lost {addr} ({error})");
            stream = connect(addr);
            listen(&stream, commands);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use melee_events::Command;

    use crate::shipper::{QUEUE_CAPACITY, Shipper};

    fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        (listener, addr)
    }

    #[test]
    fn lines_arrive_in_order_newline_delimited() {
        let (listener, addr) = listener();
        let shipper = Shipper::spawn(addr).expect("spawn");
        for i in 0..100 {
            shipper.send(format!("{{\"seq\":{i}}}"));
        }
        let (stream, _) = listener.accept().expect("accept");
        let received: Vec<String> = BufReader::new(stream)
            .lines()
            .take(100)
            .map(|line| line.expect("line"))
            .collect();
        let expected: Vec<String> = (0..100).map(|i| format!("{{\"seq\":{i}}}")).collect();
        assert_eq!(received, expected);
        assert_eq!(shipper.dropped(), 0);
    }

    #[test]
    fn events_sent_before_the_collector_is_up_are_delivered() {
        let (listener, addr) = listener();
        drop(listener);
        let shipper = Shipper::spawn(addr.clone()).expect("spawn");
        shipper.send("early".into());
        thread::sleep(Duration::from_millis(100));
        let listener = TcpListener::bind(&addr).expect("rebind");
        let (stream, _) = listener.accept().expect("accept");
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        assert_eq!(line, "early\n");
    }

    #[test]
    fn reconnects_after_the_collector_restarts() {
        let (listener, addr) = listener();
        let shipper = Shipper::spawn(addr).expect("spawn");
        shipper.send("first".into());
        let (stream, _) = listener.accept().expect("accept");
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        assert_eq!(line, "first\n");

        listener.set_nonblocking(true).expect("nonblocking");
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            shipper.send("again".into());
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Err(error) => panic!("shipper never reconnected: {error}"),
            }
        };
        stream.set_nonblocking(false).expect("blocking");
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        assert_eq!(line, "again\n");
    }

    fn commands_within(shipper: &Shipper, wanted: usize, limit: Duration) -> Vec<Command> {
        let deadline = Instant::now() + limit;
        let mut received = Vec::new();
        while received.len() < wanted && Instant::now() < deadline {
            received.extend(shipper.commands());
            thread::sleep(Duration::from_millis(5));
        }
        received
    }

    #[test]
    fn commands_from_the_collector_reach_the_inbox_in_order() {
        let (listener, addr) = listener();
        let shipper = Shipper::spawn(addr).expect("spawn");
        let (mut stream, _) = listener.accept().expect("accept");
        stream
            .write_all(
                concat!(
                    r#"{"type":"nameplate","key":"DEMO-7","summary":"Sandbag"}"#,
                    "\n",
                    "not json\n",
                    r#"{"type":"from_a_newer_sidecar"}"#,
                    "\n",
                    r#"{"type":"notice","text":"DEMO-7 closed"}"#,
                    "\n",
                )
                .as_bytes(),
            )
            .expect("write");
        assert_eq!(
            commands_within(&shipper, 2, Duration::from_secs(5)),
            [
                Command::Nameplate {
                    key: "DEMO-7".into(),
                    summary: "Sandbag".into()
                },
                Command::Notice {
                    text: "DEMO-7 closed".into()
                },
            ]
        );
    }

    #[test]
    fn commands_still_arrive_while_events_are_shipped() {
        let (listener, addr) = listener();
        let shipper = Shipper::spawn(addr).expect("spawn");
        let (stream, _) = listener.accept().expect("accept");
        let mut writer = stream.try_clone().expect("clone");
        shipper.send("event".into());
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).expect("read");
        assert_eq!(line, "event\n");
        writer
            .write_all(b"{\"type\":\"notice\",\"text\":\"hi\"}\n")
            .expect("write");
        assert_eq!(
            commands_within(&shipper, 1, Duration::from_secs(5)),
            [Command::Notice { text: "hi".into() }]
        );
    }

    #[test]
    fn an_idle_inbox_is_empty() {
        let (_listener, addr) = listener();
        let shipper = Shipper::spawn(addr).expect("spawn");
        assert_eq!(shipper.commands().count(), 0);
    }

    #[test]
    fn a_dead_collector_never_blocks_and_overflow_is_counted() {
        let (listener, addr) = listener();
        drop(listener);
        let shipper = Shipper::spawn(addr).expect("spawn");
        let total = QUEUE_CAPACITY as u64 + 500;
        let started = Instant::now();
        for i in 0..total {
            shipper.send(i.to_string());
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "send blocked the caller"
        );
        let dropped = shipper.dropped();
        assert!((499..=500).contains(&dropped), "dropped {dropped}");
    }
}
