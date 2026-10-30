// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 데몬과 REPL 워커 사이의 파이프: 한 줄에 JSON 하나. 읽기에 기한을 두고(poll) 한 줄의 길이에 상한을 둔다.
//! 워커는 믿지 않는 JS를 돌리므로 데몬은 워커가 보낸 줄을 믿지 않는다(길이, JSON 꼴, 메서드를 본다).

use std::{
    io::{self, Read, Write},
    os::fd::{AsRawFd, RawFd},
    time::Instant,
};

use serde::Serialize;

/// 읽는 쪽 파이프 (워커의 표준 입력, 데몬 쪽의 워커 표준 출력)
pub trait ReadFd: Read + AsRawFd + Send {}

impl<T: Read + AsRawFd + Send> ReadFd for T {}

/// 어느 것이든 읽는 파이프 (데몬은 워커 프로세스의 표준 출력을, 시험은 스레드의 파이프를 읽는다)
pub struct Source(pub Box<dyn ReadFd>);

impl Read for Source {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl AsRawFd for Source {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// 줄 단위로 읽는다
pub struct Lines<R> {
    inner: R,
    buffer: Vec<u8>,
    /// buffer의 앞에서 이만큼은 줄바꿈이 없다 (긴 줄을 조각마다 처음부터 다시 훑지 않는다)
    scanned: usize,
    /// 한 줄의 바이트 수 상한
    limit: usize,
}

impl<R: Read + AsRawFd> Lines<R> {
    pub fn new(inner: R, limit: usize) -> Lines<R> {
        Lines {
            inner,
            buffer: Vec::new(),
            scanned: 0,
            limit,
        }
    }

    /// 다음 줄(줄바꿈 뺀 것). 기한까지 오지 않으면 None이고, 건너편이 닫히면 UnexpectedEof다
    pub fn next(&mut self, deadline: Option<Instant>) -> io::Result<Option<String>> {
        let too_long = || io::Error::new(io::ErrorKind::InvalidData, "the line is too long");
        loop {
            let newline = self.buffer[self.scanned..]
                .iter()
                .position(|&byte| byte == b'\n');
            if let Some(found) = newline {
                let end = self.scanned + found;
                // 상한은 줄이 한 조각에 끝나도 지킨다 (검토: 상한을 조금 넘는 줄이 지나갔다)
                if end > self.limit {
                    return Err(too_long());
                }
                let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                line.pop();
                self.scanned = 0;
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"));
            }
            self.scanned = self.buffer.len();
            if self.buffer.len() > self.limit {
                return Err(too_long());
            }
            if let Some(deadline) = deadline
                && !readable(self.inner.as_raw_fd(), deadline)?
            {
                return Ok(None);
            }
            let mut chunk = [0u8; 64 * 1024];
            match self.inner.read(&mut chunk) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
    }
}

/// 메시지 하나를 한 줄로 보낸다
pub fn send(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}

/// 기한까지 읽을 것이 오는지 (건너편이 닫혀도 참: read가 0을 준다)
fn readable(fd: RawFd, deadline: Instant) -> io::Result<bool> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        // 1ms보다 짧게 남은 것을 0으로 내리면 기한까지 헛돌므로 올린다
        let ms = left.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32;
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // 안전: pollfd 하나를 넘긴다
        match unsafe { libc::poll(&raw mut poll, 1, ms) } {
            0 if Instant::now() >= deadline => return Ok(false),
            0 => {}
            ready if ready > 0 => return Ok(true),
            _ => {
                let err = io::Error::last_os_error();
                if err.kind() != io::ErrorKind::Interrupted {
                    return Err(err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn lines_come_whole_and_in_order() {
        let (reader, mut writer) = io::pipe().unwrap();
        let mut lines = Lines::new(reader, 1024);
        writer.write_all(b"{\"a\":1}\n{\"b\"").unwrap();
        let soon = || Some(Instant::now() + Duration::from_secs(1));
        assert_eq!(lines.next(soon()).unwrap().as_deref(), Some("{\"a\":1}"));
        // 반쪽 줄은 기다린다
        let started = Instant::now();
        let waited = lines
            .next(Some(Instant::now() + Duration::from_millis(50)))
            .unwrap();
        assert_eq!(waited, None);
        assert!(started.elapsed() >= Duration::from_millis(50));
        writer.write_all(b":2}\n").unwrap();
        assert_eq!(lines.next(soon()).unwrap().as_deref(), Some("{\"b\":2}"));
        send(&mut writer, &serde_json::json!({ "한글": "값" })).unwrap();
        assert_eq!(
            lines.next(None).unwrap().as_deref(),
            Some("{\"한글\":\"값\"}")
        );
        drop(writer);
        let closed = lines.next(soon()).unwrap_err();
        assert_eq!(closed.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_line_over_the_limit_is_refused() {
        let (reader, mut writer) = io::pipe().unwrap();
        let mut lines = Lines::new(reader, 16);
        writer.write_all(&[b'x'; 64]).unwrap();
        let err = lines.next(None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        // 한 조각에 끝난 줄도
        let (reader, mut writer) = io::pipe().unwrap();
        let mut lines = Lines::new(reader, 16);
        writer
            .write_all(b"0123456789abcdef\n0123456789abcdefg\n")
            .unwrap();
        assert_eq!(lines.next(None).unwrap().unwrap().len(), 16);
        let err = lines.next(None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let (reader, mut writer) = io::pipe().unwrap();
        writer.write_all(b"\xff\xfe\n").unwrap();
        let err = Lines::new(reader, 16).next(None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
