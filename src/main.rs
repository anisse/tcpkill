use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::Error as IoErr;
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6, TcpStream,
};
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
    /// List tcp connections, basically like ss -tpn
    List,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Single { pid, fd } => tcpkill(*pid as i32, *fd as i32)?,
        Commands::List => list()?,
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

struct Stream {
    local: SocketAddr,
    remote: SocketAddr,
}

impl std::fmt::Display for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\t{}",
            format_addr(self.local),
            format_addr(self.remote)
        )
    }
}
fn format_addr(addr: SocketAddr) -> String {
    format!(
        "{:>41}:{:<5}",
        match addr.ip() {
            IpAddr::V4(v4) => format!("{v4}"),
            IpAddr::V6(v6) => format!("[{v6}]"), // TODO: ignores scope
        },
        addr.port()
    )
}

fn list() -> Result<(), String> {
    let streams = streams()?;
    //let mut sockets = Vec::new();
    let pids: Vec<_> = fs::read_dir("/proc")
        .map_err(|e| format!("opening /proc: {e}"))?
        .filter_map(|res| res.ok()) // discard errors for individual files
        .map(|f| f.file_name().into_string()) // keep only basename from path, and only Strings
        .filter_map(|res| res.ok()) // valid utf-8 only
        .filter(|f| f.as_bytes().iter().all(|c| c.is_ascii_digit())) // only pids (digit-only strings)
        .collect();
    if pids.is_empty() {
        return Err("Could not list processes".to_string());
    }
    println!(
        "{:>47}\t{:>47}\t{:>8}\t{:>5}\tinode",
        "local", "remote", "pid", "fd"
    );
    for pid in pids {
        // ignore errors, /proc is inherently flaky and for all we know it could be a disappeared or different process
        if let Ok(dir) = fs::read_dir(format!("/proc/{}/fd", pid)) {
            for fd in dir.filter_map(|res| res.ok()) {
                if let Ok(link) = fs::read_link(fd.path()) {
                    if let Some(s) = link.to_str() {
                        if s.starts_with("socket:[") {
                            let inode: u64 = s
                                .strip_prefix("socket:[")
                                .ok_or("Convert error".to_string())?
                                .strip_suffix("]")
                                .ok_or("Convert error".to_string())?
                                .parse()
                                .map_err(|e| format!("Parse error: {e}"))?;
                            if let Some(stream) = streams.get(&inode) {
                                println!(
                                    "{stream}\t{pid:>8}\t{:>5}\t{inode}",
                                    fd.file_name().to_string_lossy(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn streams() -> Result<HashMap<u64, Stream>, String> {
    let mut streams = HashMap::new();
    proc_streams("/proc/net/tcp", proc_net_tcp_ipv4_parse, &mut streams)?;
    proc_streams("/proc/net/tcp6", proc_net_tcp6_ipv6_parse, &mut streams)?;

    Ok(streams)
}

fn proc_streams(
    proc_file: &str,
    ip_parse: fn(s: &str) -> Result<SocketAddr, String>,
    streams: &mut HashMap<u64, Stream>,
) -> Result<(), String> {
    for line in fs::read_to_string(proc_file)
        .map_err(|e| format!("Cannot /proc/net/tcp: {}", IoErr::from(e)))?
        .lines()
        .skip(1)
    {
        let arr: Vec<&str> = line.split_ascii_whitespace().collect();
        if arr.len() < 10 {
            return Err("Unrecognized /proc/net/tcp: expected at least 10 columns".to_string());
        }
        let local = ip_parse(arr[1])?;
        let remote = ip_parse(arr[2])?;
        let inode: u64 = arr[9]
            .parse()
            .map_err(|e| format!("cannot parse inode: {e}"))?;
        streams.insert(inode, Stream { local, remote });
    }
    Ok(())
}

fn proc_net_tcp_ipv4_parse(s: &str) -> Result<SocketAddr, String> {
    if s.len() != 8 + 1 + 4 {
        return Err("bad IPv4:port len in /proc".to_string());
    }
    let (ip, port) = match s.split_once(':') {
        Some((ip, port)) => (ip, port),
        None => return Err("bad IPv4 format /proc".to_string()),
    };
    if ip.len() != 8 {
        return Err("bad IPv4 len in /proc".to_string());
    }
    if port.len() != 4 {
        return Err("bad IPv4 port len in /proc".to_string());
    }
    let ip =
        u32::from_str_radix(ip, 16).map_err(|e| format!("failed to parse IPv4 in /proc: {e}"))?;
    let port =
        u16::from_str_radix(port, 16).map_err(|e| format!("failed to parse port in /proc: {e}"))?;
    Ok(SocketAddr::from(SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(ip)),
        port,
    )))
}

fn proc_net_tcp6_ipv6_parse(s: &str) -> Result<SocketAddr, String> {
    if s.len() != 32 + 1 + 4 {
        return Err("bad IPv6 len in /proc".to_string());
    }
    let (ip, port) = match s.split_once(':') {
        Some((ip, port)) => (ip, port),
        None => return Err("bad IPv6 format /proc".to_string()),
    };
    if ip.len() != 32 {
        return Err("bad IPv6 len in /proc".to_string());
    }
    if port.len() != 4 {
        return Err("bad IPv6 port len in /proc".to_string());
    }
    let ip =
        u128::from_str_radix(ip, 16).map_err(|e| format!("failed to parse IPv6 in /proc: {e}"))?;
    let port =
        u16::from_str_radix(port, 16).map_err(|e| format!("failed to parse port in /proc: {e}"))?;
    Ok(SocketAddr::from(SocketAddrV6::new(
        Ipv6Addr::from(v6_be_to_bytes(ip)),
        port,
        0,
        0,
    )))
}
fn v6_be_to_bytes(ip: u128) -> [u8; 16] {
    [
        ((ip >> (12 * 8)) & 0xff) as u8,
        ((ip >> (13 * 8)) & 0xff) as u8,
        ((ip >> (14 * 8)) & 0xff) as u8,
        ((ip >> (15 * 8)) & 0xff) as u8,
        ((ip >> (8 * 8)) & 0xff) as u8,
        ((ip >> (9 * 8)) & 0xff) as u8,
        ((ip >> (10 * 8)) & 0xff) as u8,
        ((ip >> (11 * 8)) & 0xff) as u8,
        ((ip >> (4 * 8)) & 0xff) as u8,
        ((ip >> (5 * 8)) & 0xff) as u8,
        ((ip >> (6 * 8)) & 0xff) as u8,
        ((ip >> (7 * 8)) & 0xff) as u8,
        ((ip >> (0 * 8)) & 0xff) as u8,
        ((ip >> (1 * 8)) & 0xff) as u8,
        ((ip >> (2 * 8)) & 0xff) as u8,
        ((ip >> (3 * 8)) & 0xff) as u8,
    ]
}
