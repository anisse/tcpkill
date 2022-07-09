mod tcpkill;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::Error as IoErr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use clap::{Parser, Subcommand};

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
        Commands::Single { pid, fd } => tcpkill::tcpkill(*pid as i32, *fd as i32)?,
        Commands::List => list()?,
    }

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
    let mut comms: HashMap<u32, String> = HashMap::new();
    for (i, sock) in SocketFdIterator::new()?.enumerate() {
        let sock = sock?;
        if i == 0 {
            println!(
                "{:>47}\t{:>47}\t{:>8}\t{:>5}\t{:>12}\tinode",
                "local", "remote", "pid", "fd", "comm",
            );
        }
        if let Some(stream) = streams.get(&sock.inode) {
            let comm: &str = comms.entry(sock.pid).or_insert_with(|| {
                fs::read_to_string(format!("/proc/{}/comm", sock.pid))
                    .unwrap_or(String::new())
                    .trim()
                    .to_string()
            });
            println!(
                "{stream}\t{:>8}\t{:>5}\t{comm:>12}\t{}",
                sock.pid, sock.fd, sock.inode
            );
        }
    }
    Ok(())
}

struct SocketFd {
    pid: u32,
    fd: u32,
    inode: u64,
}

struct SocketFdIterator {
    pids: Box<dyn Iterator<Item = String>>,
    fds: Option<fs::ReadDir>,
    pid: u32,
}

impl SocketFdIterator {
    fn new() -> Result<Self, String> {
        Ok(SocketFdIterator {
            pids: Box::new(
                fs::read_dir("/proc")
                    .map_err(|e| format!("opening /proc: {e}"))?
                    .filter_map(|res| res.ok()) // discard errors for individual files
                    .map(|f| f.file_name().into_string()) // keep only basename from path, and only Strings
                    .filter_map(|res| res.ok()) // valid utf-8 only
                    .filter(|f| f.as_bytes().iter().all(|c| c.is_ascii_digit())), // only pids (digit-only strings)
            ),
            fds: None,
            pid: 0,
        })
    }
    fn nextfd(fds: &mut Option<fs::ReadDir>, pid: u32) -> Option<Result<SocketFd, String>> {
        if let Some(dirs) = fds {
            for fd in dirs.filter_map(|res| res.ok()) {
                if let Ok(link) = fs::read_link(fd.path()) {
                    if let Some(s) = link.to_str() {
                        if s.starts_with("socket:[") {
                            fn inode(s: &str) -> Result<u64, String> {
                                Ok(s.strip_prefix("socket:[")
                                    .ok_or(format!("impossible parse error ?"))?
                                    .strip_suffix("]")
                                    .ok_or(format!("parse ] error"))?
                                    .parse()
                                    .map_err(|e| format!("Parse error: {e}"))?)
                            }
                            let inode = match inode(s) {
                                Err(x) => return Some(Err(x)),
                                Ok(x) => x,
                            };
                            return Some(Ok(SocketFd {
                                pid,
                                fd: fd.file_name().to_string_lossy().parse::<u32>().unwrap(),
                                inode,
                            }));
                        }
                    }
                }
            }
        }
        None
    }
}

impl Iterator for SocketFdIterator {
    type Item = Result<SocketFd, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(fd) = SocketFdIterator::nextfd(&mut self.fds, self.pid) {
            return Some(fd);
        }
        let (pids, fds) = (&mut *self.pids, &mut self.fds);
        for pid in pids {
            self.pid = pid.parse().unwrap();
            *fds = fs::read_dir(format!("/proc/{}/fd", pid)).ok(); //ignore open errors
            if let Some(fd) = SocketFdIterator::nextfd(fds, self.pid) {
                return Some(fd);
            }
        }
        None
    }
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
        .map_err(|e| format!("Cannot open {}: {}", proc_file, IoErr::from(e)))?
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
    let b = ip.to_le_bytes();
    [
        b[12], b[13], b[14], b[15], //
        b[8], b[9], b[10], b[11], //
        b[4], b[5], b[6], b[7], //
        b[0], b[1], b[2], b[3], //
    ]
}
