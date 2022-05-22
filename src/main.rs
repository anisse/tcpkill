use std::error::Error;
use std::io::Error as IoErr;
use std::net::{Shutdown, TcpStream};
use std::os::unix::prelude::FromRawFd;

use clap::{Parser, Subcommand};
use uapi::{getsockopt, pidfd_getfd, pidfd_open, setsockopt};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Kill a single TCP connection by PID+FD
    Single {
        /// PID that has the TCP connection
        pid: u32,
        /// Socket file descriptor number
        fd: u32,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Single { pid, fd } => tcpkill(*pid as i32, *fd as i32)?,
    }

    Ok(())
}

fn tcpkill(pid: i32, targetfd: i32) -> Result<(), String> {
    let fd = pidfd_open(pid, 0).map_err(|e| format!("Cannot open pid: {}", IoErr::from(e)))?;
    let sock = pidfd_getfd(fd.raw(), targetfd, 0).map_err(|e| match e.0 {
        uapi::c::EPERM => String::from(
            "Cannot get fd from process, \
            check that you are root or that \
            you disabled yama ptrace_scope: \
            echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope",
        ),
        _ => IoErr::from(e).to_string(),
    })?;

    let mut val: i32 = 0;
    getsockopt(sock.raw(), uapi::c::SOL_SOCKET, uapi::c::SO_TYPE, &mut val)
        .map_err(|e| format!("getsockopt error on fd: {}", IoErr::from(e)))?;
    if val != uapi::c::SOCK_STREAM {
        return Err("fd {targetfd} is not a TCP socket".to_string());
    }
    let stream = unsafe { TcpStream::from_raw_fd(sock.raw()) };

    match stream.take_error() {
        Err(x) => return Err(x.to_string()),
        Ok(Some(e)) => return Err(e.to_string()),
        Ok(None) => {}
    }
    let peer = stream
        .peer_addr()
        .map_err(|e| format!("No peer address, socket is probably not established: {e}"))?;
    let local = stream
        .local_addr()
        .map_err(|e| format!("No local address, socket is probably not established: {e}"))?;
    println!("{} --> {}", local, peer);
    /* ensures it will send an RST upon shutdown to let the other side know to close the stream
     * ugly: re-uses the fd we had previously and put in TcpStream, but this is because we don't
     * want to rely on nightly's feature(tcp_linger). We also don't want to set linger before the
     * previous checks on the socket.
     * */
    setsockopt(
        sock.raw(),
        uapi::c::SOL_SOCKET,
        uapi::c::SO_LINGER,
        &uapi::c::linger {
            l_onoff: 1,
            l_linger: 0,
        },
    )
    .map_err(|e| format!("cannot linger: {}", IoErr::from(e)))?;
    /* Forget about socket to prevent closing it twice */
    std::mem::forget(sock);
    stream
        .shutdown(Shutdown::Both)
        .map_err(|e| format!("cannot shutdown: {e}"))?;
    Ok(())
}
