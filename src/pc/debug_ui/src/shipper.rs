use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

pub const QUEUE_CAPACITY: usize = 4096;
const FIRST_RETRY: Duration = Duration::from_millis(250);
const MAX_RETRY: Duration = Duration::from_secs(5);

pub struct Shipper {
    queue: SyncSender<String>,
    dropped: Arc<AtomicU64>,
}

impl Shipper {
    pub fn spawn(addr: String) -> io::Result<Self> {
        let (queue, lines) = sync_channel(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("event-shipper".into())
            .spawn(move || ship(&addr, &lines))?;
        Ok(Self {
            queue,
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

fn ship(addr: &str, lines: &Receiver<String>) {
    let mut stream = connect(addr);
    while let Ok(mut line) = lines.recv() {
        line.push('\n');
        while let Err(error) = stream.write_all(line.as_bytes()) {
            eprintln!("events: lost {addr} ({error})");
            stream = connect(addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

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
