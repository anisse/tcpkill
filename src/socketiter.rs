use std::fs;

pub struct SocketFd {
    pub pid: u32,
    pub fd: u32,
    pub inode: u64,
}

pub struct SocketFdIterator {
    pids: Box<dyn Iterator<Item = String>>,
    fds: Option<fs::ReadDir>,
    pid: u32,
}

impl SocketFdIterator {
    pub fn new() -> Result<Self, String> {
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
