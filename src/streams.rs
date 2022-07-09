use std::collections::HashMap;
use std::fs;
use std::io::Error as IoErr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

pub struct Stream {
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

pub fn streams() -> Result<HashMap<u64, Stream>, String> {
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
