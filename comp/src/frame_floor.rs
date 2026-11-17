// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 프레임 바닥: 사람이 화면을 움직이는 동안 프레임이 정한 값(기본 초당 60) 아래로 떨어지면 GPU의 최소 클럭을
//! 올리고, 지켜지면 다시 내린다 (설계안 8.2절).
//!
//! SoC의 GPU 드라이버는 사용률만 보고 클럭을 정한다(devfreq). 스크롤처럼 앞 프레임이 나가면 곧바로 다음 프레임을
//! 그리는 일은 GPU가 느려도 사용률이 그대로다: 프레임만 준다. 그래서 프레임이 떨어져도 클럭이 오르지 않는다
//! (TB375FC: 상한 542 MHz에서 스크롤의 사용률은 55~60%로, 상한을 푸는 75%에 닿지 않는다). 프레임이 몇 장 나갔는지와
//! 사람이 쓰는 중인지는 컴포지터만 안다. 순정 안드로이드는 같은 일을 SurfaceFlinger와 GPU 정책 드라이버(GED)가 나눠
//! 한다.
//!
//! - 재는 것: 출력마다 화면에 나간 프레임(페이지 플립)의 간격. 이어진 움직임(간격 50 ms 이하)의 최근 100 ms 평균이다.
//! - 올리기: 사람이 입력한 지 3초 안이거나 컴포지터 애니메이션 중이고 평균이 바닥보다 느리면, GPU의 최소
//!   클럭(devfreq `min_freq`)을 지금 클럭에서 25% 올린다. 드라이버가 스스로 올리는 중이면 기다린다.
//! - 그만두기: 세 번 올려도 프레임이 나아지지 않으면 GPU 탓이 아니다(30프레임 영상, CPU에 묶인 화면). 나아졌던
//!   데까지만 남기고 한동안 다시 올리지 않는다.
//! - 내리기: 3초 지켜지면 한 단계 내려 본다. 그 뒤 3초 안에 프레임이 깨지면 내린 탓으로 보고 바로 되돌리며, 다음에는
//!   더 늦게 내려 본다. 움직임이 멈추거나 사람이 손을 떼고 3초가 지나면 최소 클럭을 푼다. 지켜지던 값은 10초
//!   기억해, 드라이버가 그때 머물던 클럭까지 돌아왔는데도 느리면 한 번에 그 값으로 간다.
//!
//! 최소 클럭 파일을 이 사용자가 쓸 수 없으면(기기 BSP의 udev 규칙이 연다) 아무것도 하지 않는다. x86 PC의 GPU는
//! devfreq가 없어 해당하지 않는다. 판단([`FrameFloor`])은 파일을 읽고 쓰지 않아 모형으로 시험한다.

use std::{
    collections::VecDeque,
    fs, io,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// 프레임 수를 재는 창
const WINDOW: Duration = Duration::from_millis(100);
/// 이보다 벌어진 프레임은 이어진 움직임이 아니다 (초당 20프레임 아래)
const GAP: Duration = Duration::from_millis(50);
/// 바닥보다 이만큼(%) 느려야 늦은 것이다. 시각의 흔들림을 넘긴다
const TOLERANCE_PCT: u128 = 2;
/// 한 번에 올리고 내리는 폭 (%). TB375FC의 GPU 드라이버가 한 번에 올리는 폭과 같다
const STEP_PCT: u64 = 25;
/// 지금 클럭을 읽는 간격. 두 번 이어 같아야 드라이버가 스스로 바꾸는 중이 아니다
const SAMPLE_EVERY: Duration = Duration::from_millis(50);
/// 평균 간격이 이만큼(%) 줄어야 나아진 것이다
const IMPROVED_PCT: u128 = 3;
/// 나아지지 않은 올리기가 이만큼 이어지면 그만둔다. 프레임은 화면 주기의 배수로만 나아지므로(144 Hz에서 48 →
/// 72프레임) 한두 번으로는 알 수 없다
const FUTILE_STEPS: u32 = 3;
/// 그만둔 뒤 다시 올려 보기까지. 그만둘 때마다 두 배로 늘린다
const RAISE_BACKOFF: Duration = Duration::from_secs(4);
const RAISE_BACKOFF_MAX: Duration = Duration::from_secs(64);
/// 프레임이 이만큼 지켜지면 한 단계 내려 본다
const HOLD: Duration = Duration::from_secs(3);
/// 내려 본 것이 프레임을 깬 뒤 다시 내려 보기까지. 깰 때마다 두 배로 늘린다
const LOWER_BACKOFF: Duration = Duration::from_secs(4);
const LOWER_BACKOFF_MAX: Duration = Duration::from_secs(32);
/// 내린 뒤 이 안에 프레임이 깨지면 내린 탓이다. 아슬아슬한 클럭에서는 프레임이 가끔만 밀려, 깨지는 것이 몇백
/// 밀리초 뒤에야 보인다. 짧게 잡으면 내리기와 올리기를 몇 초마다 되풀이한다
const PROBE_CHECK: Duration = HOLD;
/// 풀기 전에 지켜지던 값을 기억하는 시간
const HINT_FOR: Duration = Duration::from_secs(10);
/// 프레임이 이만큼 나가지 않으면 움직임이 멈춘 것이다: 최소 클럭을 푼다
pub const IDLE_RELEASE: Duration = Duration::from_millis(250);
/// 사람의 마지막 입력 뒤 이만큼은 사람이 화면을 움직이는 중으로 본다 (손을 뗀 뒤의 관성 스크롤)
pub const INTERACTIVE_FOR: Duration = Duration::from_secs(3);

/// 출력 하나에 나간 프레임
struct Stream {
    key: u64,
    /// 그 화면의 주기. 바닥이 화면보다 빠를 수는 없다
    refresh: Duration,
    /// 이어진 움직임의 프레임 시각. 창의 시작이나 그 앞의 마지막 프레임부터 남긴다
    flips: VecDeque<Instant>,
}

/// 늦는 동안 올려 본 기록
struct Streak {
    /// 가장 짧았던 평균 간격
    best: Duration,
    /// 그때의 바닥. 그만둘 때 여기로 돌아간다
    floor: Option<usize>,
    /// 그 뒤로 나아지지 않은 판단 수
    futile: u32,
}

/// 프레임 바닥의 판단. 프레임 시각과 GPU의 지금 클럭을 받아 최소 클럭을 정한다
pub struct FrameFloor {
    /// 바닥의 프레임 간격
    target: Duration,
    /// GPU가 낼 수 있는 클럭 (Hz, 오름차순)
    opps: Vec<u64>,
    /// 올릴 수 있는 가장 높은 칸
    top: usize,
    streams: Vec<Stream>,
    /// 지금 건 최소 클럭(opps의 칸). None이면 걸지 않았다
    floor: Option<usize>,
    /// 지금 바닥에서 프레임이 지켜졌다
    held: bool,
    /// 마지막으로 읽은 GPU 클럭과 그때
    sample: Option<(u64, Instant)>,
    /// 이어 읽은 두 값이 같다
    stable: bool,
    /// 늦음을 마지막으로 따진 때. 창 하나에 한 번만 따진다
    judged: Option<Instant>,
    streak: Option<Streak>,
    raise_backoff: Duration,
    raise_blocked: Option<Instant>,
    /// 프레임이 지켜지기 시작한 때
    kept_since: Option<Instant>,
    /// 내려 보기 전의 칸과 내린 때
    probe: Option<(usize, Instant)>,
    lower_backoff: Duration,
    lower_blocked: Option<Instant>,
    /// 바닥을 올리기 시작할 때 드라이버가 스스로 머물던 칸
    raised_from: Option<usize>,
    /// 풀기 전에 지켜지던 칸, 그 바닥을 올리기 시작할 때 드라이버가 머물던 칸, 푼 때
    hint: Option<(usize, usize, Instant)>,
}

impl FrameFloor {
    /// `min_fps`가 0이거나 올릴 클럭이 없으면 None. `max_mhz`는 올릴 수 있는 가장 높은 클럭(0이면 GPU의 최고
    /// 클럭까지), `frequencies`는 GPU가 낼 수 있는 클럭(Hz)이다
    pub fn new(min_fps: u32, max_mhz: u32, mut frequencies: Vec<u64>) -> Option<Self> {
        frequencies.sort_unstable();
        frequencies.dedup();
        if min_fps == 0 || frequencies.len() < 2 {
            return None;
        }
        let top = if max_mhz == 0 {
            frequencies.len() - 1
        } else {
            let max = u64::from(max_mhz) * 1_000_000;
            frequencies.iter().rposition(|hz| *hz <= max)?
        };
        if top == 0 {
            return None;
        }
        Some(Self {
            target: Duration::from_nanos(1_000_000_000 / u64::from(min_fps)),
            opps: frequencies,
            top,
            streams: Vec::new(),
            floor: None,
            held: false,
            sample: None,
            stable: false,
            judged: None,
            streak: None,
            raise_backoff: RAISE_BACKOFF,
            raise_blocked: None,
            kept_since: None,
            probe: None,
            lower_backoff: LOWER_BACKOFF,
            lower_blocked: None,
            raised_from: None,
            hint: None,
        })
    }

    /// 지금 건 최소 클럭 (Hz)
    pub fn floor_hz(&self) -> Option<u64> {
        self.floor.map(|index| self.opps[index])
    }

    /// 올릴 수 있는 가장 높은 클럭 (Hz)
    pub fn top_hz(&self) -> u64 {
        self.opps[self.top]
    }

    /// 프레임 하나가 화면에 나갔다. `key`는 출력이고 `refresh`는 그 화면의 주기다
    pub fn frame(&mut self, key: u64, refresh: Duration, now: Instant) {
        let index = match self.streams.iter().position(|stream| stream.key == key) {
            Some(index) => index,
            None => {
                self.streams.push(Stream {
                    key,
                    refresh,
                    flips: VecDeque::new(),
                });
                self.streams.len() - 1
            }
        };
        let stream = &mut self.streams[index];
        stream.refresh = refresh;
        if stream
            .flips
            .back()
            .is_some_and(|last| now.saturating_duration_since(*last) > GAP)
        {
            stream.flips.clear();
        }
        stream.flips.push_back(now);
        while stream.flips.len() > 2 && now.saturating_duration_since(stream.flips[1]) >= WINDOW {
            stream.flips.pop_front();
        }
    }

    /// 프레임이 나간 뒤 부른다. 최소 클럭을 바꿔야 하면 그 값(Hz, 0이면 풀기)을 낸다. `interactive`는 사람이 화면을
    /// 움직이는 중인지, `current`는 GPU의 지금 클럭(Hz)을 읽는 함수다. 읽지 못하면 아무것도 하지 않는다
    pub fn judge(
        &mut self,
        now: Instant,
        interactive: bool,
        current: &mut dyn FnMut() -> Option<u64>,
    ) -> Option<u64> {
        if !interactive {
            return self.release(now);
        }
        if self
            .sample
            .is_none_or(|(_, at)| now.saturating_duration_since(at) >= SAMPLE_EVERY)
        {
            let hz = current()?;
            self.stable = self.sample.is_some_and(|(last, _)| last == hz);
            self.sample = Some((hz, now));
        }
        let (mean, target) = self.pace(now)?;
        if mean.as_nanos() * 100 > target.as_nanos() * (100 + TOLERANCE_PCT) {
            self.late(now, mean)
        } else {
            self.kept(now)
        }
    }

    /// 사람이 쓰지 않거나 움직임이 멈췄다: 최소 클럭을 푼다. 프레임이 지켜지던 값은 잠깐 기억한다
    pub fn release(&mut self, now: Instant) -> Option<u64> {
        for stream in &mut self.streams {
            stream.flips.clear();
        }
        self.streak = None;
        self.kept_since = None;
        self.judged = None;
        self.sample = None;
        self.stable = false;
        // 내려 보던 중이면 지켜지던 것은 내리기 전의 값이다
        let probed = self.probe.take().map(|(from, _)| from);
        let raised_from = self.raised_from.take();
        let floor = self.floor.take()?;
        if self.held {
            self.hint = Some((probed.unwrap_or(floor), raised_from.unwrap_or(0), now));
        }
        self.held = false;
        Some(0)
    }

    /// 가장 느린 출력의 평균 프레임 간격과 그 출력의 바닥 간격. 창을 채운 이어진 움직임이 없으면 None
    fn pace(&self, now: Instant) -> Option<(Duration, Duration)> {
        self.streams
            .iter()
            .filter_map(|stream| {
                let (first, last) = (*stream.flips.front()?, *stream.flips.back()?);
                let frames = u32::try_from(stream.flips.len() - 1).ok()?;
                let span = last.saturating_duration_since(first);
                if frames == 0 || span < WINDOW || now.saturating_duration_since(last) > GAP {
                    return None;
                }
                Some((span / frames, self.target.max(stream.refresh)))
            })
            // 바닥에 견줘 가장 느린 것: mean / target이 가장 큰 것
            .max_by_key(|(mean, target)| mean.as_nanos() * 1_000_000 / target.as_nanos().max(1))
    }

    fn late(&mut self, now: Instant, mean: Duration) -> Option<u64> {
        self.kept_since = None;
        self.held = false;
        // 내려 본 것이 프레임을 깼다: 되돌리고 다음에는 더 늦게 내려 본다
        if let Some((from, at)) = self.probe.take()
            && now.saturating_duration_since(at) <= PROBE_CHECK
        {
            self.lower_blocked = Some(now + self.lower_backoff);
            self.lower_backoff = (self.lower_backoff * 2).min(LOWER_BACKOFF_MAX);
            return self.set(Some(from));
        }
        if !self.stable
            || self
                .judged
                .is_some_and(|at| now.saturating_duration_since(at) < WINDOW)
            || self.raise_blocked.is_some_and(|until| now < until)
        {
            return None;
        }
        self.judged = Some(now);
        let (sample, _) = self.sample?;
        match &mut self.streak {
            None => {
                self.streak = Some(Streak {
                    best: mean,
                    floor: self.floor,
                    futile: 0,
                });
            }
            Some(streak) => {
                if mean.as_nanos() * 100 <= streak.best.as_nanos() * (100 - IMPROVED_PCT) {
                    *streak = Streak {
                        best: mean,
                        floor: self.floor,
                        futile: 0,
                    };
                } else if self.floor != streak.floor {
                    streak.futile += 1;
                }
            }
        }
        // 올려도 나아지지 않는다: 나아졌던 데까지만 남기고 한동안 올리지 않는다
        if let Some(streak) = self.streak.take_if(|streak| streak.futile >= FUTILE_STEPS) {
            self.raise_blocked = Some(now + self.raise_backoff);
            self.raise_backoff = (self.raise_backoff * 2).min(RAISE_BACKOFF_MAX);
            return self.set(streak.floor);
        }
        // 드라이버가 스스로 고른 칸과, 바닥까지 넣은 지금의 칸
        let own = self.at_least(sample);
        let base = own.max(self.floor.unwrap_or(0));
        let mut next = self.step_up(base);
        // 방금 지켜지던 값이 있으면 한 번에 그 값으로 간다. 드라이버가 그때 머물던 클럭까지 돌아온 뒤에만: 그
        // 아래에서 느린 것은 드라이버가 아직 오르는 중이어서일 수 있다(가벼운 화면이면 거기서 지켜진다)
        match self.hint {
            Some((_, _, at)) if now.saturating_duration_since(at) > HINT_FOR => self.hint = None,
            Some((hint, from, _)) if base >= from => {
                next = next.max(hint);
                self.hint = None;
            }
            _ => {}
        }
        let next = next.min(self.top);
        if next <= base {
            return None;
        }
        // 드라이버가 바닥 없이 머물던 가장 높은 칸을 적어 둔다. 오르는 도중에 한 칸 밀어 준 것은 그 뒤 드라이버가
        // 더 오르면 덮어쓴다
        if self.floor.is_none_or(|floor| own > floor) {
            self.raised_from = Some(own);
        }
        self.set(Some(next))
    }

    fn kept(&mut self, now: Instant) -> Option<u64> {
        if self.streak.take().is_some() {
            self.raise_backoff = RAISE_BACKOFF;
        }
        self.held = true;
        if self
            .probe
            .is_some_and(|(_, at)| now.saturating_duration_since(at) > PROBE_CHECK)
        {
            self.probe = None;
            self.lower_backoff = LOWER_BACKOFF;
        }
        let floor = self.floor?;
        // 드라이버가 바닥보다 위에서 돌면 바닥은 하는 일이 없다
        if self.stable && self.sample.is_some_and(|(hz, _)| hz > self.opps[floor]) {
            self.probe = None;
            return self.set(None);
        }
        let since = *self.kept_since.get_or_insert(now);
        if self.probe.is_some()
            || now.saturating_duration_since(since) < HOLD
            || self.lower_blocked.is_some_and(|until| now < until)
        {
            return None;
        }
        self.probe = Some((floor, now));
        self.kept_since = None;
        self.set(self.step_down(floor))
    }

    /// 최소 클럭을 바꾼다. 바꾼 뒤의 프레임만 재도록 창을 비우고 클럭도 다시 읽는다
    fn set(&mut self, floor: Option<usize>) -> Option<u64> {
        if floor == self.floor {
            return None;
        }
        self.floor = floor;
        if floor.is_none() {
            self.raised_from = None;
        }
        self.sample = None;
        self.stable = false;
        for stream in &mut self.streams {
            let last = stream.flips.pop_back();
            stream.flips.clear();
            stream.flips.extend(last);
        }
        Some(floor.map_or(0, |index| self.opps[index]))
    }

    /// 그 클럭 이상인 가장 낮은 칸
    fn at_least(&self, hz: u64) -> usize {
        self.opps
            .iter()
            .position(|opp| *opp >= hz)
            .unwrap_or(self.opps.len() - 1)
    }

    /// 한 단계 위: STEP_PCT 안에서 가장 높은 칸. 그 안에 없으면 바로 위 칸
    fn step_up(&self, index: usize) -> usize {
        let limit = self.opps[index] + self.opps[index] * STEP_PCT / 100;
        let within = self
            .opps
            .iter()
            .rposition(|opp| *opp <= limit)
            .unwrap_or(index);
        within.max(index + 1).min(self.opps.len() - 1)
    }

    /// 한 단계 아래. 가장 낮은 클럭까지 내려가면 바닥을 푼다(None)
    fn step_down(&self, index: usize) -> Option<usize> {
        let limit = self.opps[index] * 100 / (100 + STEP_PCT);
        let lower = self.at_least(limit).min(index.saturating_sub(1));
        (lower > 0).then_some(lower)
    }
}

/// GPU의 devfreq 파일. 닫을 때 최소 클럭을 푼다
pub struct Devfreq {
    dir: PathBuf,
    min: fs::File,
}

impl Devfreq {
    /// 렌더 노드(문자 장치의 주 번호와 부 번호)의 GPU에서 찾는다. devfreq가 없거나 최소 클럭을 이 사용자가 쓸 수
    /// 없으면 None
    pub fn open(major: u32, minor: u32) -> Option<Self> {
        Self::open_in(Path::new("/sys/dev/char"), major, minor)
    }

    fn open_in(char_devices: &Path, major: u32, minor: u32) -> Option<Self> {
        let parent = char_devices
            .join(format!("{major}:{minor}"))
            .join("device/devfreq");
        let mut dirs: Vec<PathBuf> = fs::read_dir(parent)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join("min_freq").exists())
            .collect();
        dirs.sort();
        let dir = dirs.into_iter().next()?;
        let min = fs::OpenOptions::new()
            .write(true)
            .open(dir.join("min_freq"))
            .ok()?;
        Some(Self { dir, min })
    }

    /// GPU가 낼 수 있는 클럭 (Hz)
    pub fn frequencies(&self) -> Vec<u64> {
        fs::read_to_string(self.dir.join("available_frequencies"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|hz| hz.parse().ok())
            .collect()
    }

    /// 지금 클럭 (Hz)
    pub fn current(&self) -> Option<u64> {
        fs::read_to_string(self.dir.join("cur_freq"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// 최소 클럭 (Hz). 0이면 푼다
    pub fn set_min(&self, hz: u64) -> io::Result<()> {
        self.min.write_all_at(format!("{hz}\n").as_bytes(), 0)
    }
}

impl Drop for Devfreq {
    fn drop(&mut self) {
        let _ = self.set_min(0);
    }
}

/// 컴포지터가 쥐는 프레임 바닥: 판단과 GPU의 파일
pub struct GpuFloor {
    floor: FrameFloor,
    devfreq: Devfreq,
    /// 마지막 프레임이 나간 때. 움직임이 멈췄는지 본다
    last_frame: Option<Instant>,
    /// 최소 클럭을 쓰지 못했다. 다시 쓰려 하지 않는다
    broken: bool,
}

impl GpuFloor {
    /// 렌더 노드의 GPU에 프레임 바닥을 건다. GPU에 devfreq가 없거나, 최소 클럭을 쓸 수 없거나, `min_fps`가 0이면
    /// None. 앞의 컴포지터가 남긴 최소 클럭이 있으면 푼다
    pub fn open(major: u32, minor: u32, min_fps: u32, max_mhz: u32) -> Option<Self> {
        if min_fps == 0 {
            return None;
        }
        let devfreq = Devfreq::open(major, minor)?;
        let floor = FrameFloor::new(min_fps, max_mhz, devfreq.frequencies())?;
        devfreq.set_min(0).ok()?;
        Some(Self {
            floor,
            devfreq,
            last_frame: None,
            broken: false,
        })
    }

    /// 올릴 수 있는 가장 높은 클럭 (MHz)
    pub fn top_mhz(&self) -> u64 {
        self.floor.top_hz() / 1_000_000
    }

    /// 프레임 하나가 화면에 나갔다. 최소 클럭을 걸고 있으면 true다: 움직임이 멈추는 것을 타이머로 봐야 한다
    /// ([`Self::idle`])
    pub fn frame(&mut self, key: u64, refresh: Duration, now: Instant, interactive: bool) -> bool {
        if self.broken {
            return false;
        }
        self.last_frame = Some(now);
        self.floor.frame(key, refresh, now);
        let devfreq = &self.devfreq;
        let change = self
            .floor
            .judge(now, interactive, &mut || devfreq.current());
        self.apply(change, now);
        self.floor.floor_hz().is_some()
    }

    /// 타이머가 부른다. 프레임이 [`IDLE_RELEASE`] 동안 나가지 않았으면 최소 클럭을 풀고 None, 아직이면 다시 볼
    /// 때까지 남은 시간을 낸다
    pub fn idle(&mut self, now: Instant) -> Option<Duration> {
        self.floor.floor_hz()?;
        let since = self
            .last_frame
            .map_or(IDLE_RELEASE, |at| now.saturating_duration_since(at));
        if since < IDLE_RELEASE {
            return Some(IDLE_RELEASE - since);
        }
        self.release(now);
        None
    }

    /// 최소 클럭을 푼다 (세션을 잃을 때)
    pub fn release(&mut self, now: Instant) {
        let change = self.floor.release(now);
        self.apply(change, now);
    }

    fn apply(&mut self, change: Option<u64>, now: Instant) {
        let Some(hz) = change else {
            return;
        };
        tracing::debug!("GPU minimum {} MHz", hz / 1_000_000);
        if let Err(err) = self.devfreq.set_min(hz) {
            tracing::warn!("frame floor off: cannot set the GPU minimum frequency: {err}");
            self.broken = true;
            self.floor.release(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MHZ: u64 = 1_000_000;
    /// 144 Hz 화면의 주기
    const VBLANK: Duration = Duration::from_nanos(6_944_444);

    /// TB375FC의 GPU처럼 265~1400 MHz를 20 MHz쯤의 칸으로 나눈 표
    fn opps() -> Vec<u64> {
        let low = [
            265, 284, 304, 324, 344, 363, 383, 403, 423, 442, 462, 482, 502, 522, 542, 562, 582,
            601, 621, 640, 660, 680, 701, 720, 740, 760, 780, 800, 820, 840, 860, 880, 900, 920,
            940, 960, 980, 1000, 1013,
        ];
        let high = (1040..=1400).step_by(20);
        low.into_iter().chain(high).map(|mhz| mhz * MHZ).collect()
    }

    /// 드라이버의 폴링 간격과 평소의 상한 (TB375FC)
    const POLL: Duration = Duration::from_millis(50);
    const CEILING: u64 = 542 * MHZ;

    /// 드라이버가 스스로 고른 클럭(`own`)과 컴포지터가 건 최소 클럭. `rule`을 켜면 TB375FC의 GPU 드라이버처럼 50 ms마다
    /// 사용률로 `own`을 정한다(패치 0201~0203): 50%를 넘으면 한 번에 25%씩 상한(542 MHz, 최소 클럭이 더 높으면 그
    /// 값)까지 올리고, 40% 아래면 사용률이 45%가 되는 클럭으로 내린다. 상한이 풀리는 것(75%가 2초)은 넣지 않았다
    struct Gpu {
        own: u64,
        min: u64,
        rule: bool,
        /// 이번 폴링 창에서 GPU가 일한 시간, 창이 시작한 때(시험 시작부터), 지난 창의 사용률
        busy: Duration,
        window: Duration,
        load: u64,
    }

    impl Gpu {
        fn clock(&self) -> u64 {
            if self.rule {
                self.own
            } else {
                self.own.max(self.min)
            }
        }

        /// GPU가 `busy` 동안 일했고 시각이 `now`(시험 시작부터)가 됐다. 지난 폴링 창마다 클럭을 다시 정한다
        fn ran(&mut self, busy: Duration, now: Duration) {
            self.busy += busy;
            while self.rule && now >= self.window + POLL {
                self.load = (self.busy.as_nanos() * 100 / POLL.as_nanos()).min(100) as u64;
                self.busy = Duration::ZERO;
                self.window += POLL;
                self.decide();
            }
        }

        /// 컴포지터가 최소 클럭을 썼다. 드라이버는 그 자리에서 다시 정한다
        fn set_min(&mut self, min: u64) {
            self.min = min;
            if self.rule {
                self.decide();
            }
        }

        fn decide(&mut self) {
            let opps = opps();
            let top = opps[opps.len() - 1];
            let want = if self.load > 50 {
                top
            } else if self.load > 40 {
                self.own
            } else {
                self.own * self.load / 45
            };
            // devfreq가 최소 클럭 아래로 두지 않고, 드라이버의 상한은 최소 클럭이 더 높으면 그 값이다
            let mut want = want.max(self.min).min(CEILING.max(self.min));
            // 한 번에 25%까지만 올린다. 그 안에 칸이 없으면 바로 위 칸
            let limit = self.own + self.own / 4;
            if want > limit {
                want = opps
                    .iter()
                    .rev()
                    .find(|opp| **opp <= limit && **opp > self.own)
                    .or(opps.iter().find(|opp| **opp > self.own))
                    .copied()
                    .unwrap_or(top);
            }
            self.own = opps
                .iter()
                .find(|opp| **opp >= want)
                .copied()
                .unwrap_or(top);
        }
    }

    /// 앞 프레임이 나가면 곧바로 다음 프레임을 그리는 화면. 프레임 하나의 일은 CPU 시간과 GPU 일(클럭 사이클)이고,
    /// 다 그린 뒤의 첫 화면 주기에 나간다. `every`가 있으면 내용이 그 간격으로만 나온다(영상)
    struct Scene {
        cpu: Duration,
        cycles: u64,
        every: Option<Duration>,
    }

    /// 시험 하나의 시계와 기록
    struct Run {
        origin: Instant,
        now: Instant,
        floor: FrameFloor,
        gpu: Gpu,
        /// 바꾼 최소 클럭 (시작부터의 시간, MHz)
        changes: Vec<(Duration, u64)>,
        /// 프레임 간격
        intervals: Vec<(Duration, Duration)>,
        /// 프레임마다 CPU 시간을 이만큼까지 흔든다
        jitter: Duration,
        seed: u64,
    }

    impl Run {
        fn new(own_mhz: u64) -> Self {
            let origin = Instant::now();
            Self {
                origin,
                now: origin,
                floor: FrameFloor::new(60, 0, opps()).unwrap(),
                gpu: Gpu {
                    own: own_mhz * MHZ,
                    min: 0,
                    rule: false,
                    busy: Duration::ZERO,
                    window: Duration::ZERO,
                    load: 0,
                },
                changes: Vec::new(),
                intervals: Vec::new(),
                jitter: Duration::ZERO,
                seed: 1,
            }
        }

        /// 쉬던 GPU(265 MHz)에서 시작하고 드라이버가 스스로 클럭을 정한다
        fn with_driver() -> Self {
            let mut run = Self::new(265);
            run.gpu.rule = true;
            run
        }

        /// 0부터 `jitter`의 두 배까지 고르게 (선형 합동 난수, 시험마다 같은 차례)
        fn noise(&mut self) -> Duration {
            self.seed = self
                .seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let span = self.jitter.as_nanos() as u64 * 2;
            Duration::from_nanos(if span == 0 {
                0
            } else {
                (self.seed >> 33) % span
            })
        }

        /// `scene`을 `length` 동안 그린다
        fn play(&mut self, scene: &Scene, length: Duration, interactive: bool) {
            let end = self.now + length;
            loop {
                let gpu_time =
                    Duration::from_nanos(scene.cycles * 1_000_000_000 / self.gpu.clock());
                let work = (scene.cpu + self.noise()).saturating_sub(self.jitter) + gpu_time;
                let ready = self.now + work.max(scene.every.unwrap_or_default());
                let since = ready.duration_since(self.origin).as_nanos();
                let vblanks = since.div_ceil(VBLANK.as_nanos());
                let flip = self.origin + Duration::from_nanos((vblanks * VBLANK.as_nanos()) as u64);
                if flip > end {
                    self.now = end;
                    return;
                }
                self.intervals.push((
                    flip.duration_since(self.origin),
                    flip.duration_since(self.now),
                ));
                self.now = flip;
                self.gpu.ran(gpu_time, flip.duration_since(self.origin));
                self.floor.frame(1, VBLANK, flip);
                let clock = self.gpu.clock();
                if let Some(min) = self.floor.judge(flip, interactive, &mut || Some(clock)) {
                    self.gpu.set_min(min);
                    self.changes
                        .push((flip.duration_since(self.origin), min / MHZ));
                }
            }
        }

        /// 움직임이 `length` 동안 멈춘다. 컴포지터의 타이머가 최소 클럭을 푼다
        fn rest(&mut self, length: Duration) {
            if length >= IDLE_RELEASE
                && let Some(min) = self.floor.release(self.now + IDLE_RELEASE)
            {
                self.gpu.set_min(min);
                self.changes.push((
                    self.now.duration_since(self.origin) + IDLE_RELEASE,
                    min / MHZ,
                ));
            }
            self.now += length;
            // 쉬는 GPU는 사용률이 0이라 드라이버가 가장 낮은 클럭으로 내린다
            if self.gpu.rule && length >= POLL * 2 {
                self.gpu.own = opps()[0];
                self.gpu.busy = Duration::ZERO;
                self.gpu.load = 0;
            }
            self.gpu.window = self.now.duration_since(self.origin);
        }

        /// `from` 뒤로 나간 프레임의 초당 수
        fn fps_since(&self, from: Duration) -> f64 {
            let frames: Vec<_> = self.intervals.iter().filter(|(at, _)| *at > from).collect();
            let total: Duration = frames.iter().map(|(_, interval)| *interval).sum();
            frames.len() as f64 / total.as_secs_f64()
        }

        /// `from` 뒤로 바닥보다 느렸던 프레임의 시간
        fn late_since(&self, from: Duration) -> Duration {
            self.intervals
                .iter()
                .filter(|(at, interval)| *at > from && *interval > Duration::from_micros(17_000))
                .map(|(_, interval)| *interval)
                .sum()
        }

        fn min_mhz(&self) -> u64 {
            self.gpu.min / MHZ
        }
    }

    const fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// 2026-11-08 TB375FC에서 잰 Firefox 손가락 스크롤: 542 MHz에서 72프레임
    fn light_scroll() -> Scene {
        Scene {
            cpu: Duration::from_micros(5200),
            cycles: 4_200_000,
            every: None,
        }
    }

    /// 2026-11-05의 무거운 시험 페이지: 프레임 시간 5.2 ms + 8067/MHz ms. 542 MHz에서 48프레임
    fn heavy_page() -> Scene {
        Scene {
            cpu: Duration::from_micros(5200),
            cycles: 8_067_000,
            every: None,
        }
    }

    #[test]
    fn a_scroll_that_keeps_its_frames_is_left_alone() {
        let mut run = Run::new(542);
        run.play(&light_scroll(), ms(5000), true);
        assert_eq!(run.changes, []);
        assert!(run.fps_since(ms(0)) > 71.0, "{}", run.fps_since(ms(0)));
    }

    #[test]
    fn a_heavy_page_gets_the_clock_it_needs() {
        let mut run = Run::new(542);
        run.play(&heavy_page(), ms(1500), true);
        // 542 → 660 → 820 → 1013 MHz: 세 번째에서 화면 주기 둘(72프레임)에 든다
        let raised: Vec<u64> = run.changes.iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(raised, [660, 820, 1013]);
        assert!(run.changes[2].0 < ms(700), "{:?}", run.changes);
        assert!(run.fps_since(ms(700)) > 71.0, "{}", run.fps_since(ms(700)));
        assert_eq!(run.floor.floor_hz(), Some(1013 * MHZ));
    }

    #[test]
    fn lowering_that_breaks_the_frames_is_undone_and_tried_later() {
        let mut run = Run::new(542);
        run.play(&heavy_page(), ms(12_000), true);
        let after: Vec<(Duration, u64)> = run
            .changes
            .iter()
            .filter(|(at, _)| *at > ms(1000))
            .copied()
            .collect();
        // 3초 지켜진 뒤 820으로 내려 봤다가 0.4초 안에 1013으로 되돌린다. 다음은 4초 뒤, 그다음은 8초 뒤다
        let values: Vec<u64> = after.iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(values, [820, 1013, 820, 1013], "{after:?}");
        assert!(after[1].0 - after[0].0 <= PROBE_CHECK, "{after:?}");
        assert!(after[2].0 - after[1].0 >= ms(4000), "{after:?}");
        assert_eq!(run.min_mhz(), 1013);
        // 내려 본 두 번(각 0.2초쯤)만 바닥 아래였다
        assert!(
            run.late_since(ms(1000)) < ms(500),
            "{:?}",
            run.late_since(ms(1000))
        );
    }

    #[test]
    fn the_clock_comes_down_when_the_page_gets_lighter() {
        let mut run = Run::new(542);
        run.play(&heavy_page(), ms(1500), true);
        assert_eq!(run.min_mhz(), 1013);
        let from = run.now.duration_since(run.origin);
        run.play(&light_scroll(), ms(14_000), true);
        // 3초마다 한 단계: 820, 660, 542, 그다음에는 드라이버가 스스로 542에 있으므로 푼다
        let values: Vec<u64> = run
            .changes
            .iter()
            .filter(|(at, _)| *at > from)
            .map(|(_, mhz)| *mhz)
            .collect();
        assert_eq!(values, [820, 660, 542, 442, 0]);
        assert_eq!(run.late_since(from), ms(0));
        assert_eq!(run.floor.floor_hz(), None);
    }

    #[test]
    fn a_video_is_not_chased() {
        // 30프레임 영상: 프레임은 내용이 정한다. GPU 일은 적다
        let video = Scene {
            cpu: Duration::from_micros(3000),
            cycles: 1_500_000,
            every: Some(Duration::from_micros(33_300)),
        };
        // 손대지 않고 보는 동안에는 아무것도 하지 않는다
        let mut run = Run::new(304);
        run.play(&video, ms(10_000), false);
        assert_eq!(run.changes, []);
        // 화면을 만지는 동안: 세 번 올려 보고 나아지지 않으니 풀고, 4초·8초 뒤에야 다시 해 본다
        let mut run = Run::new(304);
        run.play(&video, ms(20_000), true);
        let values: Vec<u64> = run.changes.iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(
            values,
            [363, 442, 542, 0, 363, 442, 542, 0, 363, 442, 542, 0]
        );
        assert!(run.changes.iter().all(|(_, mhz)| *mhz <= 542));
        let gaps: Vec<Duration> = run
            .changes
            .chunks(4)
            .map(|attempt| attempt[0].0)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        assert!(gaps[0] >= ms(4000) && gaps[1] >= ms(8000), "{gaps:?}");
        assert_eq!(run.min_mhz(), 0);
    }

    #[test]
    fn it_keeps_what_helped_when_the_floor_is_out_of_reach() {
        // 최고 클럭에서도 60프레임이 안 되는 화면: 1013 MHz에서 48프레임이 되고 그 위로는 그대로다
        let scene = Scene {
            cpu: Duration::from_micros(5200),
            cycles: 14_000_000,
            every: None,
        };
        let mut run = Run::new(542);
        run.play(&scene, ms(3000), true);
        let values: Vec<u64> = run.changes.iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(values, [660, 820, 1013, 1260, 1400, 1013]);
        assert_eq!(run.min_mhz(), 1013);
        let fps = run.fps_since(ms(2000));
        assert!((47.0..49.0).contains(&fps), "{fps}");
    }

    #[test]
    fn the_floor_goes_when_the_hand_leaves_and_comes_back_at_once() {
        let mut run = Run::new(542);
        run.play(&heavy_page(), ms(1500), true);
        assert_eq!(run.min_mhz(), 1013);
        // 사람이 손을 떼고 3초가 지났다(interactive가 거짓): 다음 프레임에서 푼다
        let before = run.changes.len();
        run.play(&heavy_page(), ms(200), false);
        assert_eq!(run.changes[before..].len(), 1);
        assert_eq!(run.min_mhz(), 0);
        // 움직임이 멈췄다가 다시 스크롤한다: 지켜지던 값으로 한 번에 간다
        run.rest(ms(2000));
        let before = run.changes.len();
        run.play(&heavy_page(), ms(1000), true);
        let values: Vec<u64> = run.changes[before..].iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(values, [1013]);
        // 기억은 10초까지다
        run.rest(ms(11_000));
        let before = run.changes.len();
        run.play(&heavy_page(), ms(1500), true);
        let values: Vec<u64> = run.changes[before..].iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(values, [660, 820, 1013]);
    }

    #[test]
    fn stopping_releases_the_floor() {
        let mut run = Run::new(542);
        run.play(&heavy_page(), ms(1500), true);
        assert_eq!(run.min_mhz(), 1013);
        run.rest(ms(300));
        assert_eq!(run.min_mhz(), 0);
        assert_eq!(run.floor.floor_hz(), None);
    }

    #[test]
    fn it_waits_while_the_driver_is_still_raising() {
        // 스크롤을 시작하면 드라이버가 265 MHz에서 50 ms마다 25%씩 스스로 올린다. 그동안의 느린 프레임으로 올리지 않는다
        let mut run = Run::new(265);
        for own in [265, 324, 403, 502, 542] {
            run.gpu.own = own * MHZ;
            run.play(&light_scroll(), ms(50), true);
        }
        run.play(&light_scroll(), ms(2000), true);
        assert_eq!(run.changes, []);
    }

    #[test]
    fn from_a_standing_start_the_driver_ramps_first() {
        // 가벼운 스크롤: 드라이버가 265 MHz에서 542 MHz까지 스스로 오르고 프레임 바닥은 손대지 않는다
        let mut run = Run::with_driver();
        run.play(&light_scroll(), ms(5000), true);
        // 드라이버가 사용률 50%에서 403 MHz에 멈추면(48프레임) 한 칸 밀어 주고, 드라이버가 그 위로 가면 푼다.
        // 상한 위로는 올리지 않는다
        let values: Vec<u64> = run.changes.iter().map(|(_, mhz)| *mhz).collect();
        assert_eq!(values, [502, 0], "{:?}", run.changes);
        assert_eq!(run.gpu.own, 542 * MHZ);
        assert_eq!(run.floor.floor_hz(), None);
        assert!(run.fps_since(ms(500)) > 71.0, "{}", run.fps_since(ms(500)));
        // 무거운 페이지: 드라이버가 상한에 닿아 멈춘 뒤부터 올린다
        let mut run = Run::with_driver();
        run.play(&heavy_page(), ms(2000), true);
        let values: Vec<u64> = run.changes.iter().map(|(_, mhz)| *mhz).collect();
        // 드라이버가 오르다 한 창 멈춘 사이에 상한 아래에서 한 칸 밀어 줄 수는 있다. 상한 위로는 그 뒤에만 올린다
        assert!(values.ends_with(&[660, 820, 1013]), "{:?}", run.changes);
        let above: Vec<(Duration, u64)> = run
            .changes
            .iter()
            .filter(|(_, mhz)| *mhz > 542)
            .copied()
            .collect();
        assert!(above[0].0 >= ms(250), "{:?}", run.changes);
        assert!(above[2].0 <= ms(900), "{:?}", run.changes);
        assert!(
            run.changes
                .iter()
                .all(|(at, mhz)| *mhz > 542 || *at < above[0].0),
            "{:?}",
            run.changes
        );
        assert!(
            run.fps_since(ms(1000)) > 71.0,
            "{}",
            run.fps_since(ms(1000))
        );
        // 손을 뗐다가 다시 스크롤하면 드라이버가 다시 오른 뒤 한 번에 간다
        run.rest(ms(1000));
        let before = run.changes.len();
        let from = run.now.duration_since(run.origin);
        run.play(&heavy_page(), ms(1500), true);
        let again: Vec<(Duration, u64)> = run.changes[before..]
            .iter()
            .filter(|(_, mhz)| *mhz > 542)
            .copied()
            .collect();
        assert_eq!(again.len(), 1, "{:?}", &run.changes[before..]);
        assert_eq!(again[0].1, 1013);
        assert!(again[0].0 - from <= ms(450), "{again:?}");
    }

    #[test]
    fn a_light_page_after_a_heavy_one_is_not_pushed_to_the_remembered_clock() {
        let mut run = Run::with_driver();
        run.play(&heavy_page(), ms(2000), true);
        assert_eq!(run.min_mhz(), 1013);
        run.rest(ms(1000));
        // 드라이버가 다시 오르는 동안 프레임이 잠깐 느리다. 기억한 1013 MHz로 뛰지 않고, 상한 아래에서만 민다
        let before = run.changes.len();
        run.play(&light_scroll(), ms(3000), true);
        assert!(
            run.changes[before..].iter().all(|(_, mhz)| *mhz <= 542),
            "{:?}",
            &run.changes[before..]
        );
        assert_eq!(run.floor.floor_hz(), None);
        assert_eq!(run.gpu.own, 542 * MHZ);
    }

    #[test]
    fn changing_content_does_not_make_it_chatter() {
        // 40초 동안 2초마다 무거운 화면과 가벼운 화면이 바뀌고, 프레임마다 CPU 시간이 ±1 ms 흔들린다. 무거운
        // 화면은 1013 MHz에서 아슬아슬하다(프레임의 일이 12.2~14.2 ms, 화면 주기 둘은 13.9 ms)
        let mut run = Run::with_driver();
        run.jitter = ms(1);
        for _ in 0..10 {
            run.play(&heavy_page(), ms(2000), true);
            run.play(&light_scroll(), ms(2000), true);
        }
        let late = run.late_since(ms(1000));
        let fps = run.fps_since(ms(1000));
        let summary = format!(
            "{} changes, late {late:?}, {fps:.1} fps: {:?}",
            run.changes.len(),
            run.changes
        );
        // 바꾸는 것은 2초에 한 번을 넘지 않는다
        assert!(run.changes.len() <= 20, "{summary}");
        // 처음 올라가는 1초 뒤로, 화면 주기 셋에 나간 프레임의 시간은 전체의 5%를 넘지 않는다
        assert!(late < ms(2000), "{summary}");
        assert!(fps > 69.0, "{summary}");
    }

    #[test]
    fn the_cap_and_the_refresh_rate_bound_the_floor() {
        // 올릴 수 있는 클럭을 800 MHz로 묶으면 그 위로 가지 않는다
        let mut run = Run::new(542);
        run.floor = FrameFloor::new(60, 800, opps()).unwrap();
        assert_eq!(run.floor.top_hz(), 800 * MHZ);
        run.play(&heavy_page(), ms(1500), true);
        assert!(
            run.changes.iter().all(|(_, mhz)| *mhz <= 800),
            "{:?}",
            run.changes
        );
        // 30 Hz 화면에서는 30프레임이 바닥이다
        let mut floor = FrameFloor::new(60, 0, opps()).unwrap();
        let start = Instant::now();
        let mut changed = None;
        for frame in 0..60u32 {
            let now = start + Duration::from_micros(33_333) * frame;
            floor.frame(7, Duration::from_micros(33_333), now);
            changed = changed.or(floor.judge(now, true, &mut || Some(542 * MHZ)));
        }
        assert_eq!(changed, None);
        // 끄거나 올릴 칸이 없으면 만들지 않는다
        assert!(FrameFloor::new(0, 0, opps()).is_none());
        assert!(FrameFloor::new(60, 0, vec![265 * MHZ]).is_none());
        assert!(FrameFloor::new(60, 200, opps()).is_none());
    }

    #[test]
    fn the_slowest_screen_decides() {
        let mut floor = FrameFloor::new(60, 0, opps()).unwrap();
        let start = Instant::now();
        let mut changes = Vec::new();
        // 화면 1은 72프레임, 화면 2는 48프레임
        let mut flips: Vec<(Instant, u64)> = (0..40u32)
            .map(|n| (start + VBLANK * 2 * n, 1))
            .chain((0..27u32).map(|n| (start + VBLANK * 3 * n, 2)))
            .collect();
        flips.sort();
        for (now, key) in flips {
            floor.frame(key, VBLANK, now);
            changes.extend(floor.judge(now, true, &mut || Some(542 * MHZ)));
        }
        assert_eq!(changes.first(), Some(&(660 * MHZ)));
    }

    #[test]
    fn devfreq_files_are_found_under_the_render_node() {
        let root = std::env::temp_dir().join(format!("bunt-frame-floor-{}", std::process::id()));
        let dir = root.join("226:128/device/devfreq/13000000.gpu");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("available_frequencies"),
            "542000000 265000000 1400000000 \n",
        )
        .unwrap();
        fs::write(dir.join("cur_freq"), "542000000\n").unwrap();
        fs::write(dir.join("min_freq"), "0\n").unwrap();
        assert!(Devfreq::open_in(&root, 226, 129).is_none());
        let devfreq = Devfreq::open_in(&root, 226, 128).unwrap();
        let mut frequencies = devfreq.frequencies();
        frequencies.sort_unstable();
        assert_eq!(frequencies, [265 * MHZ, 542 * MHZ, 1400 * MHZ]);
        assert_eq!(devfreq.current(), Some(542 * MHZ));
        devfreq.set_min(660 * MHZ).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("min_freq")).unwrap().trim(),
            "660000000"
        );
        // 닫으면 푼다 (앞의 값보다 짧게 써도 파일의 뒤는 커널에서는 남지 않는다. 여기서는 첫 줄만 본다)
        drop(devfreq);
        let left = fs::read_to_string(dir.join("min_freq")).unwrap();
        assert_eq!(left.lines().next(), Some("0"));
        let _ = fs::remove_dir_all(&root);
    }
}
