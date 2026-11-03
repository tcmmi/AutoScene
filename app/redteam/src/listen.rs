//! The listeners the attack pages aim at, all on 127.0.0.1, so nothing a
//! page sends leaves the machine:
//! - the wall's, for HTTP: a request it hears got past the CSP or the
//!   sandbox, and fails the run. A connection that sends nothing is kept
//!   apart (`opened`): a webview may open one to where a page was about to
//!   go before the CSP stops the request, which the CSP can't close;
//! - the side one, for HTTP, and one for UDP: what the CSP doesn't cover
//!   (link hints, WebRTC's STUN).
//!
//! What isn't a request at the wall is measured per engine, never a
//! failure.
//!
//! Each records what it heard with the check that was running.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

/// Which listener heard something.
#[derive(Clone, Copy)]
pub enum Ear {
    Wall,
    /// A connection to the wall that sent no request.
    Opened,
    Side,
    Udp,
}

/// Something a listener heard.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// The check that was running.
    pub check: String,
    /// When, in ms since the run started.
    pub at_ms: u64,
    /// A request's first line, "a connection, no request" for a socket
    /// that sent none, or a datagram's size.
    pub what: String,
}

/// What the listeners heard, and the check that is running.
pub struct Heard {
    started: Instant,
    check: Mutex<String>,
    pub wall: Mutex<Vec<Hit>>,
    pub opened: Mutex<Vec<Hit>>,
    pub side: Mutex<Vec<Hit>>,
    pub udp: Mutex<Vec<Hit>>,
}

impl Heard {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            check: Mutex::new("start".into()),
            wall: Mutex::default(),
            opened: Mutex::default(),
            side: Mutex::default(),
            udp: Mutex::default(),
        }
    }

    /// The check `name` starts: what is heard from now on is its.
    pub fn begin(&self, name: &str) {
        *self.check.lock() = name.to_owned();
    }

    /// The check that is running.
    pub fn check(&self) -> String {
        self.check.lock().clone()
    }

    /// What `ear` heard while the check `name` ran.
    pub fn during(&self, ear: Ear, name: &str) -> Vec<String> {
        self.of(ear).lock().iter().filter(|h| h.check == name).map(|h| h.what.clone()).collect()
    }

    pub fn of(&self, ear: Ear) -> &Mutex<Vec<Hit>> {
        match ear {
            Ear::Wall => &self.wall,
            Ear::Opened => &self.opened,
            Ear::Side => &self.side,
            Ear::Udp => &self.udp,
        }
    }

    fn hit(&self, ear: Ear, what: String) {
        let hit = Hit { check: self.check(), at_ms: self.started.elapsed().as_millis() as u64, what };
        self.of(ear).lock().push(hit);
    }
}

/// Listens for HTTP on a free port of 127.0.0.1 for `ear` (the wall or the
/// side); gives its address (`http://127.0.0.1:<port>`). Each connection
/// is recorded with its request's first line, and answered with nothing.
pub fn http(heard: Arc<Heard>, ear: Ear) -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let at = format!("http://{}", listener.local_addr()?);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let heard = heard.clone();
            thread::spawn(move || {
                let mut stream = stream;
                match (first_line(&mut stream), ear) {
                    (Some(line), ear) => heard.hit(ear, line),
                    (None, Ear::Wall) => heard.hit(Ear::Opened, "a connection, no request".into()),
                    (None, ear) => heard.hit(ear, "a connection, no request".into()),
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            });
        }
    });
    Ok(at)
}

/// The first line of what `stream` sends within 2 s, if it sends one.
fn first_line(stream: &mut TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut got = Vec::new();
    let mut buf = [0; 1024];
    while got.len() < 8192 && !got.windows(2).any(|w| w == b"\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    let line = String::from_utf8_lossy(&got).lines().next().unwrap_or_default().trim().to_owned();
    (!line.is_empty()).then_some(line)
}

/// Listens for UDP on a free port of 127.0.0.1; gives the port. Each
/// datagram is recorded by its size, and whether it is a STUN request.
pub fn udp(heard: Arc<Heard>) -> std::io::Result<u16> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    thread::spawn(move || {
        let mut buf = [0; 2048];
        while let Ok((n, _)) = socket.recv_from(&mut buf) {
            // A STUN message: a type with its first two bits clear, then the magic cookie.
            let stun = n >= 20 && buf[0] & 0xc0 == 0 && buf[4..8] == [0x21, 0x12, 0xa4, 0x42];
            heard.hit(Ear::Udp, format!("{n} bytes{}", if stun { " of STUN" } else { "" }));
        }
    });
    Ok(port)
}
