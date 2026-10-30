// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! REPL 워커를 가둔다(worker.rs). JS 엔진(QuickJS, C)에 메모리 버그가 있어 JS가 엔진 밖으로 나와도, 워커는 데몬과
//! 잇는 파이프로 말하는 것 말고는 하지 못한다. 데몬(접근성 버스와 컴포지터의 에이전트 API를 가진 프로세스)은 워커가
//! 파이프로 청한 호출만 정책에 맞춰 대신 한다.
//!
//! - 상속한 파일 기술자를 닫는다. systemd가 넘긴 에이전트 소켓(fd 3)은 close-on-exec가 아니라 워커에 남는다
//! - 자원 상한: 코어 덤프 없음(스냅샷으로 읽은 화면 내용이 메모리에 있다), 새 프로세스 없음, 파일 쓰기 없음, 주소 공간
//! - seccomp: 시스템 호출을 허용 목록(파이프 읽고 쓰기, 메모리, 시간, 끝내기)으로 줄이고 나머지는 EPERM으로
//!   거절한다. 파일 열기, 소켓, 프로세스 띄우기, 다른 프로세스에 신호 보내기, ioctl(TIOCSTI)이 모두 막힌다. 다른
//!   아키텍처의 호출(x86-64에서 int 0x80으로 부르는 i386 호출)은 프로세스를 죽인다

use std::io;

/// 워커의 주소 공간 상한. QuickJS 힙 상한(worker.rs의 MEMORY_LIMIT)보다 훨씬 크다: 실행 파일과 라이브러리, 스택,
/// 할당기의 예약이 든다. 엔진이 상한을 넘겨 할당하려는 버그가 데몬 컴퓨터의 메모리를 다 쓰지 못하게 막는다
const ADDRESS_SPACE: u64 = 1 << 30;

/// 상속한 파일 기술자 가운데 표준 입출력(0, 1, 2) 말고 모두 닫는다
pub fn close_inherited_fds() {
    // 안전: 정수 인자만 넘기는 시스템 호출이다. 다른 스레드가 없다
    let closed = unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0;
    if !closed {
        // close_range가 없는 커널(5.9 전)
        for fd in 3..1024 {
            // 안전: 열리지 않은 번호를 닫으면 EBADF로 끝난다
            unsafe { libc::close(fd) };
        }
    }
}

/// 자원 상한을 둔다. seccomp보다 먼저 한다(setrlimit도 막히므로)
pub fn limit_resources() -> io::Result<()> {
    set_limit(libc::RLIMIT_CORE, 0)?;
    set_limit(libc::RLIMIT_FSIZE, 0)?;
    set_limit(libc::RLIMIT_NPROC, 0)?;
    set_limit(libc::RLIMIT_AS, ADDRESS_SPACE)
}

fn set_limit(resource: libc::__rlimit_resource_t, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // 안전: 지역 변수 하나를 읽기만 한다
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// 허용하는 시스템 호출. 워커가 JS를 돌리며 쓰는 것뿐이다: 파이프(0, 1, 2) 읽고 쓰기, 기다리기, 메모리, 시간,
/// 신호에서 돌아오기, 끝내기. strace로 본 워커의 호출에서 골랐다
const ALLOWED: &[libc::c_long] = &[
    libc::SYS_read,
    libc::SYS_write,
    libc::SYS_readv,
    libc::SYS_writev,
    libc::SYS_close,
    libc::SYS_ppoll,
    #[cfg(target_arch = "x86_64")]
    libc::SYS_poll,
    libc::SYS_brk,
    libc::SYS_mmap,
    libc::SYS_munmap,
    libc::SYS_mremap,
    libc::SYS_madvise,
    libc::SYS_mprotect,
    libc::SYS_futex,
    libc::SYS_clock_gettime,
    libc::SYS_clock_getres,
    libc::SYS_gettimeofday,
    libc::SYS_clock_nanosleep,
    libc::SYS_nanosleep,
    libc::SYS_getrandom,
    libc::SYS_sched_yield,
    libc::SYS_getpid,
    libc::SYS_gettid,
    libc::SYS_rt_sigreturn,
    libc::SYS_rt_sigprocmask,
    libc::SYS_sigaltstack,
    libc::SYS_restart_syscall,
    libc::SYS_exit,
    libc::SYS_exit_group,
];

/// seccomp_data의 아키텍처 값 (linux/audit.h의 AUDIT_ARCH_*)
#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7;

/// classic BPF 명령 (linux/filter.h): 32비트 값 읽기(BPF_LD | BPF_W | BPF_ABS), 같으면 뛰기(BPF_JMP | BPF_JEQ |
/// BPF_K), 돌려주기(BPF_RET | BPF_K)
const LOAD_WORD: u16 = 0x20;
const JUMP_IF_EQUAL: u16 = 0x15;
const RETURN: u16 = 0x06;
/// struct seccomp_data { int nr; __u32 arch; … }
const NR_OFFSET: u32 = 0;
const ARCH_OFFSET: u32 = 4;

// 비교 줄의 뛰는 거리(jt)는 u8이다
const _: () = assert!(ALLOWED.len() < 255);

fn statement(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: JUMP_IF_EQUAL,
        jt,
        jf,
        k,
    }
}

/// 허용 목록 필터: 아키텍처가 다르면 죽이고, 목록에 있으면 허용하고, 없으면 EPERM
fn program() -> Vec<libc::sock_filter> {
    let count = ALLOWED.len();
    let mut program = vec![
        statement(LOAD_WORD, ARCH_OFFSET),
        jump(ARCH, 1, 0),
        statement(RETURN, libc::SECCOMP_RET_KILL_PROCESS),
        statement(LOAD_WORD, NR_OFFSET),
    ];
    for (at, number) in ALLOWED.iter().enumerate() {
        // 맞으면 남은 비교와 거절 줄을 건너뛰어 허용 줄로 간다
        program.push(jump(*number as u32, (count - at) as u8, 0));
    }
    program.push(statement(
        RETURN,
        libc::SECCOMP_RET_ERRNO | (libc::EPERM as u32 & libc::SECCOMP_RET_DATA),
    ));
    program.push(statement(RETURN, libc::SECCOMP_RET_ALLOW));
    program
}

/// 부른 스레드(워커는 스레드가 하나다)와 그 뒤에 띄울 스레드를 허용 목록 필터로 가둔다. 되돌릴 수 없다
pub fn confine() -> io::Result<()> {
    // 안전: 정수 인자만 넘긴다. 권한을 얻는 exec(setuid)를 막는 것은 seccomp를 쓰는 조건이다
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut filter = program();
    let program = libc::sock_fprog {
        len: filter.len() as libc::c_ushort,
        filter: filter.as_mut_ptr(),
    };
    // 안전: 커널이 필터를 복사해 간다. filter는 이 호출이 끝날 때까지 살아 있다
    if unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &raw const program,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };

    use super::*;

    /// seccomp는 스레드마다 걸린다: 새 스레드만 가두고 시험 프로세스의 나머지는 그대로 둔다. 워커처럼 파이프로 말한다
    #[test]
    fn a_confined_thread_can_only_talk_through_its_pipes() {
        let (mut from_test, mut to_worker) = std::io::pipe().unwrap();
        let (mut from_worker, mut to_test) = std::io::pipe().unwrap();
        let worker = std::thread::spawn(move || {
            confine().unwrap();
            let errno = |result: io::Result<()>| result.map_err(|err| err.raw_os_error());
            let file = errno(std::fs::File::open("/etc/hostname").map(drop));
            let socket = errno(UnixStream::connect("/run/dbus/system_bus_socket").map(drop));
            let tcp = errno(std::net::TcpStream::connect("127.0.0.1:9").map(drop));
            let child = errno(std::process::Command::new("/bin/true").spawn().map(drop));
            // 열린 파이프, 메모리, 시간, 잠자기는 된다
            let memory = vec![7u8; 8 << 20]
                .iter()
                .map(|&b| b as usize)
                .sum::<usize>();
            let started = Instant::now();
            std::thread::sleep(Duration::from_millis(5));
            let slept = started.elapsed() >= Duration::from_millis(5);
            let mut buffer = [0u8; 4];
            from_test.read_exact(&mut buffer).unwrap();
            to_test.write_all(b"pong").unwrap();
            (file, socket, tcp, child, memory, slept, buffer)
        });
        to_worker.write_all(b"ping").unwrap();
        let mut answer = [0u8; 4];
        from_worker.read_exact(&mut answer).unwrap();
        let (file, socket, tcp, child, memory, slept, buffer) = worker.join().unwrap();
        assert_eq!(file, Err(Some(libc::EPERM)));
        assert_eq!(socket, Err(Some(libc::EPERM)));
        assert_eq!(tcp, Err(Some(libc::EPERM)));
        assert!(child.is_err(), "{child:?}");
        assert_eq!(memory, 7 * (8 << 20));
        assert!(slept);
        assert_eq!((&buffer, &answer), (b"ping", b"pong"));
        // 시험 프로세스의 다른 스레드는 갇히지 않았다
        assert!(std::fs::File::open("/proc/self/status").is_ok());
    }

    #[test]
    fn the_filter_allows_exactly_the_list() {
        let program = program();
        assert_eq!(program.len(), 4 + ALLOWED.len() + 2);
        // 마지막 비교에서 맞으면 거절 줄 하나를 건너뛰어 허용 줄로 간다
        let last = &program[3 + ALLOWED.len()];
        assert_eq!((last.code, last.jt, last.jf), (JUMP_IF_EQUAL, 1, 0));
        assert_eq!(program.last().unwrap().k, libc::SECCOMP_RET_ALLOW);
        for forbidden in [
            libc::SYS_openat,
            libc::SYS_socket,
            libc::SYS_connect,
            libc::SYS_execve,
            libc::SYS_clone,
            libc::SYS_kill,
            libc::SYS_ioctl,
            libc::SYS_ptrace,
            libc::SYS_prctl,
        ] {
            assert!(!ALLOWED.contains(&forbidden), "{forbidden}");
        }
    }
}
