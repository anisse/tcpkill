#![feature(tcp_linger)]

use std::process::exit;
use std::{env, os::unix::prelude::FromRawFd};
use uapi::{getsockopt, pidfd_getfd, pidfd_open};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        let name = args.get(0).expect("Called without argv[0]");
        println!("Usage: {name} <pid> <sock-fd>");
        exit(1);
    }
    let pid: uapi::c::pid_t = args[1].parse().expect("PID not integer");
    let targetfd: uapi::c::c_int = args[2].parse().expect("Target socket fd not integer");

    let fd = pidfd_open(pid, 0).expect("Cannot open pid");
    let sock = pidfd_getfd(fd.raw(), targetfd, 0).expect("Cannot get fd from process, check that you are root or that you disabled yama ptrace_scope: echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope");

    let mut val: i32 = 0;
    getsockopt(sock.raw(), uapi::c::SOL_SOCKET, uapi::c::SO_TYPE, &mut val)
        .expect("Cannot getsockopt on this fd");
    if val != uapi::c::SOCK_STREAM {
        println!("fd {targetfd} is not a TCP socket");
        exit(2);
    }
    let stream = unsafe { std::net::TcpStream::from_raw_fd(sock.raw()) };
    println!(
        "{} --> {}",
        stream.local_addr().expect("no local addr"),
        stream.peer_addr().expect("no peer addr")
    );
    /* ensures it will send an RST upon shutdown to let the other side know to close the stream */
    stream
        .set_linger(Some(std::time::Duration::from_secs(0)))
        .expect("cannot set linger");
    stream
        .shutdown(std::net::Shutdown::Both)
        .expect("cannot shutdown");
}
