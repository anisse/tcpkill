mod socketiter;
mod streams;
mod tcpkill;

use std::collections::HashMap;
use std::error::Error;
use std::fs;

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

fn list() -> Result<(), String> {
    let streams = streams::streams()?;
    let mut comms: HashMap<u32, String> = HashMap::new();
    for (i, sock) in socketiter::SocketFdIterator::new()?.enumerate() {
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
