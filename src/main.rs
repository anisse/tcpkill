#![feature(tcp_linger)]

use std::process::exit;
use std::{env, os::unix::prelude::FromRawFd};
use uapi::{getsockname, getsockopt, pidfd_getfd, pidfd_open, shutdown};

#[derive(Default)]
struct SockAddr {
    sa_family: u16,
    _pad: [u8; 26],
}
unsafe impl uapi::SockAddr for SockAddr {}
unsafe impl uapi::Pod for SockAddr {}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        let name = args.get(0).unwrap();
        println!("Usage: {name} <pid> <sock-fd>");
        exit(1);
    }
    let pid: uapi::c::pid_t = args[1].parse().expect("PID not integer");
    let targetfd: uapi::c::c_int = args[2].parse().expect("Target socket fd not integer");

    let fd = pidfd_open(pid, 0).expect("Cannot open pid");
    println!("got fd {}", fd.raw());
    let sock = pidfd_getfd(fd.raw(), targetfd, 0).expect("Cannot get fd from process, check that you are root or that you disabled yama ptrace_scope: echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope");
    println!("got sock {}", sock.raw());

    let mut val: i32 = 0;
    getsockopt(sock.raw(), uapi::c::SOL_SOCKET, uapi::c::SO_TYPE, &mut val)
        .expect("Cannot getsockopt");
    if val != uapi::c::SOCK_STREAM {
        println!("expected TCP socket");
        exit(2);
    }
    let stream = unsafe { std::net::TcpStream::from_raw_fd(sock.raw()) };
    println!(
        "{} --> {}",
        stream.local_addr().expect("no local addr"),
        stream.peer_addr().expect("no peer addr")
    );
    stream
        .set_linger(Some(std::time::Duration::from_secs(0)))
        .expect("cannot set linger");
    stream
        .shutdown(std::net::Shutdown::Both)
        .expect("cannot shutdown");
    //shutdown(sock.raw(), uapi::c::SHUT_RDWR).expect("cannot shutdown");
    exit(0);
    /*
    let mut saddr = SockAddr::default();
    getsockname(sock.raw(), &mut saddr).expect("Cannot get sockname");
    match saddr.sa_family as i32 {
        uapi::c::AF_INET => {
            let saddr4: &uapi::c::sockaddr_in = unsafe { std::mem::transmute::<_, _>(&saddr) };
            println!(
                "v4: {}:{}",
                std::net::Ipv4Addr::from(saddr4.sin_addr.s_addr),
                u16::from_be(saddr4.sin_port)
            );
        }
        uapi::c::AF_INET6 => {
            let saddr6: uapi::c::sockaddr_in6 = unsafe { std::mem::transmute::<_, _>(saddr) };
            println!(
                "v6: [{}]:{}",
                std::net::IpAddr::from(saddr6.sin6_addr.s6_addr),
                u16::from_be(saddr6.sin6_port)
            );
        }
        _ => {
            println!("unsupported address family: {}", saddr.sa_family);
            exit(3);
        }
    }
    */
}
