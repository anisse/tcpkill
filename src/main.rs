#![feature(tcp_linger)]

use std::error::Error;
use std::io::Error as ioErr;
use std::{env, os::unix::prelude::FromRawFd};
use uapi::{getsockopt, pidfd_getfd, pidfd_open};

fn tcpkill(pid: i32, targetfd: i32) -> Result<(), String> {
    let fd = pidfd_open(pid, 0).map_err(|e| format!("Cannot open pid: {}", ioErr::from(e)))?;
    let sock = pidfd_getfd(fd.raw(), targetfd, 0).map_err(|e| match e.0 {
        uapi::c::EPERM => String::from(
            "Cannot get fd from process, \
            check that you are root or that \
            you disabled yama ptrace_scope: \
            echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope",
        ),
        _ => ioErr::from(e).to_string(),
    })?;

    let mut val: i32 = 0;
    getsockopt(sock.raw(), uapi::c::SOL_SOCKET, uapi::c::SO_TYPE, &mut val)
        .map_err(|e| format!("getsockopt error on fd: {}", ioErr::from(e)))?;
    if val != uapi::c::SOCK_STREAM {
        return Err("fd {targetfd} is not a TCP socket".to_string());
    }
    let stream = unsafe { std::net::TcpStream::from_raw_fd(sock.raw()) };

    if let Ok(local) = stream.local_addr() {
        if let Ok(peer) = stream.peer_addr() {
            println!("{} --> {}", local, peer);
        }
    }
    /* ensures it will send an RST upon shutdown to let the other side know to close the stream */
    stream
        .set_linger(Some(std::time::Duration::from_secs(0)))
        .map_err(|e| format!("cannot linger: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Both)
        .map_err(|e| format!("cannot shutdown: {e}"))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        let name = args.get(0).ok_or("Called without argv[0]")?;
        return Err(format!("Usage: {name} <pid> <sock-fd>").into());
    }
    let pid = args[1]
        .parse()
        .map_err(|_| "First argument (PID) is not an integer".to_string())?;
    let targetfd = args[2]
        .parse()
        .map_err(|_| "Second argument (target socket fd) is not an integer".to_string())?;

    tcpkill(pid, targetfd)?;

    Ok(())
}
