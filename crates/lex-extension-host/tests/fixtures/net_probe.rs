//! Network-probe test fixture for the sandbox suite.
//!
//! Attempts a TCP connect to the address passed in argv[1]. Exits 0
//! on success, 42 on any error. Tests pass a localhost listener's
//! address so the negative control (NullSandbox: exit 0) works on
//! offline CI runners.
//!
//! Usage: net_probe <host:port>

use std::net::TcpStream;
use std::time::Duration;

const EXIT_BLOCKED: i32 = 42;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

fn main() {
    let addr = std::env::args().nth(1).expect("missing addr argv");
    let parsed: std::net::SocketAddr = addr.parse().expect("addr must be host:port");
    match TcpStream::connect_timeout(&parsed, CONNECT_TIMEOUT) {
        Ok(_) => std::process::exit(0),
        Err(_) => std::process::exit(EXIT_BLOCKED),
    }
}
