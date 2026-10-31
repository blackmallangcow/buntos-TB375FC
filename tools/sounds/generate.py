# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""buntOS의 경고음과 시동음(XDG 사운드 테마 "buntos")을 만든다. 표준 라이브러리만 쓴다 (빌드 VM에 numpy가 없다).

사용법: generate.py <출력 디렉터리> [소리 이름...]
출력: <출력 디렉터리>/index.theme, <출력 디렉터리>/stereo/<소리 이름>.wav (48 kHz, 16비트 스테레오 PCM)

남이 만든 소리를 쓰지 않고 이 스크립트로 합성하므로 소리도 이 저장소와 같은 라이선스다. 소리마다 배음을 더해
음을 만들고(배음마다 감쇠가 다르다), 왼쪽과 오른쪽을 조금 다르게 조율·지연해 폭을 주고, Freeverb 꼴의 잔향을
더한 뒤 사람이 듣는 크기(K 가중 음량)를 맞춘다. 이름은 freedesktop 사운드 이름이다: 경고음 일곱 개(시스템 설정의 사운드 페이지가 고른다)와
부팅을 마치고 로그인 화면이 처음 뜰 때 내는 시동음(system-ready). 시동음은 Apple의 시동음(F장조 화음을 한꺼번에
울린다)과 겹치지 않게 D♭장조 add9 화음을 아래에서 위로 퉁긴다.
"""

import math
import os
import struct
import sys
import wave
from array import array

RATE = 48000
# 소리의 가장 큰 값의 한계 (dBFS)
PEAK_DB = -1.5
# 소리마다 듣는 크기: K 가중 음량(LUFS)의 가장 큰 값. 가장 작게 들리는 둥(경고)과 시동음은 가장 큰 값의 한계에
# 닿고, 나머지는 이만큼으로 줄여 고르게 들린다
LOUDNESS = -8.5
# ITU-R BS.1770의 K 가중 필터 (48 kHz, 쌍2차 필터의 b와 a): 머리의 영향을 본뜬 고역 선반, 저역 차단
K_WEIGHTING = (
    ((1.53512485958697, -2.69169618940638, 1.19839281085285), (-1.69065929318241, 0.73248077421585)),
    ((1.0, -2.0, 1.0), (-1.99004745483398, 0.99007225036621)),
)

# 유리처럼 맑은 음: (배수, 세기, 감쇠 시간의 배율). 거의 배음이고 2.76배의 비배음이 반짝임을 준다
GLASS = [(1.0, 1.0, 1.0), (2.0, 0.32, 0.55), (3.0, 0.1, 0.35), (2.76, 0.07, 0.4), (4.0, 0.04, 0.25)]
# 작은 종: 비배음이 많고 오래 운다
BELL = [(1.0, 1.0, 1.0), (2.0, 0.42, 0.7), (2.76, 0.3, 0.6), (4.07, 0.14, 0.4), (5.4, 0.1, 0.3)]
# 시동음의 따뜻한 음: 배음 위주로 부드럽다
WARM = [(1.0, 1.0, 1.0), (2.0, 0.42, 0.6), (3.0, 0.16, 0.42), (4.0, 0.06, 0.3), (2.003, 0.08, 0.9)]


def note(name):
    """음 이름(C4, Db5, F#6 등)의 주파수 (A4 = 440 Hz)"""
    steps = {"C": -9, "D": -7, "E": -5, "F": -4, "G": -2, "A": 0, "B": 2}
    letter, rest = name[0], name[1:]
    shift = 0
    while rest and rest[0] in "#b":
        shift += 1 if rest[0] == "#" else -1
        rest = rest[1:]
    octave = int(rest)
    return 440.0 * 2 ** ((steps[letter] + shift + 12 * (octave - 4)) / 12)


class Track:
    """스테레오 버퍼 (초 단위 길이)"""

    def __init__(self, seconds):
        count = int(seconds * RATE)
        self.left = array("d", [0.0]) * count
        self.right = array("d", [0.0]) * count

    def tone(self, start, freq, partials, decay, amp=1.0, attack=0.006, pan=0.0, sweep=None):
        """음 하나를 더한다. decay: 기본음의 감쇠 시간(초, 1/e). pan: -1(왼쪽)~1(오른쪽).
        sweep: (처음 주파수 배율, 시간 상수 초)면 처음 높이에서 freq로 미끄러진다"""
        # 왼쪽과 오른쪽을 ±1.5센트 다르게 조율하고 오른쪽을 0.25ms 늦춰 폭을 준다
        for channel, cents, delay, gain in (
            (self.left, -1.5, 0, math.sqrt(0.5 * (1 - pan))),
            (self.right, 1.5, int(0.00025 * RATE), math.sqrt(0.5 * (1 + pan))),
        ):
            first = int(start * RATE) + delay
            last = min(len(channel), first + int(decay * 7 * RATE))
            ratio = 2 ** (cents / 1200)
            for multiple, strength, scale in partials:
                base = freq * multiple * ratio
                if base >= RATE / 2 * 0.9:
                    continue
                tau = decay * scale
                level = amp * gain * strength
                phase = 0.0
                for index in range(first, last):
                    t = (index - first) / RATE
                    envelope = math.exp(-t / tau)
                    if t < attack:
                        envelope *= 0.5 - 0.5 * math.cos(math.pi * t / attack)
                    if envelope < 1e-4 and t > attack:
                        break
                    current = base
                    if sweep:
                        current = base * (1 + (sweep[0] - 1) * math.exp(-t / sweep[1]))
                    phase += 2 * math.pi * current / RATE
                    channel[index] += level * envelope * math.sin(phase)

    def reverb(self, wet, room=0.8, damp=0.35):
        """Freeverb 꼴의 잔향을 섞는다 (왼쪽과 오른쪽은 지연을 조금 달리한다)"""
        scale = RATE / 44100
        combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]
        allpasses = [556, 441, 341, 225]
        for channel, spread in ((self.left, 0), (self.right, 23)):
            dry = channel[:]
            out = array("d", [0.0]) * len(dry)
            for length in combs:
                size = int((length + spread) * scale)
                buffer = [0.0] * size
                store = 0.0
                position = 0
                for index, value in enumerate(dry):
                    held = buffer[position]
                    store = held * (1 - damp) + store * damp
                    buffer[position] = value * 0.015 + store * room
                    position = position + 1 if position + 1 < size else 0
                    out[index] += held
            for length in allpasses:
                size = int((length + spread) * scale)
                buffer = [0.0] * size
                position = 0
                for index, value in enumerate(out):
                    held = buffer[position]
                    out[index] = held - value
                    buffer[position] = value + held * 0.5
                    position = position + 1 if position + 1 < size else 0
            for index, value in enumerate(out):
                channel[index] = dry[index] + wet * 3 * value

    def loudness(self, window=0.2):
        """듣는 크기: K 가중 음량(LUFS)의 가장 큰 값. 경고음은 짧으므로 BS.1770의 순간 음량(400ms 창)보다 짧은
        창으로 잰다"""
        weighted = []
        for values in (self.left, self.right):
            for (b0, b1, b2), (a1, a2) in K_WEIGHTING:
                out = array("d", [0.0]) * len(values)
                x1 = x2 = y1 = y2 = 0.0
                for index, value in enumerate(values):
                    y = b0 * value + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
                    x2, x1, y2, y1 = x1, value, y1, y
                    out[index] = y
                values = out
            weighted.append(values)
        total = [0.0]
        for left, right in zip(*weighted):
            total.append(total[-1] + left * left + right * right)
        count, size = len(total) - 1, int(window * RATE)
        energy = max(
            total[min(count, start + size)] - total[start]
            for start in range(0, max(1, count - size + 1), int(0.01 * RATE))
        )
        return -0.691 + 10 * math.log10(energy / size) if energy > 0 else -120.0

    def finish(self, fade=0.08):
        """크기를 맞추고(듣는 크기는 LOUDNESS까지, 가장 큰 값은 PEAK_DB까지), 끝의 거의 조용한 부분(가장 큰 값보다
        55dB 아래)을 잘라 내고, 끝을 부드럽게 줄인다"""
        peak = max(max(map(abs, self.left)), max(map(abs, self.right))) or 1.0
        gain = min(10 ** (PEAK_DB / 20) / peak, 10 ** ((LOUDNESS - self.loudness()) / 20))
        quiet = peak * 10 ** (-55 / 20)
        end = len(self.left)
        while end > 0 and abs(self.left[end - 1]) < quiet and abs(self.right[end - 1]) < quiet:
            end -= 1
        end = min(len(self.left), end + int(0.02 * RATE))
        fade_samples = int(fade * RATE)
        for channel in (self.left, self.right):
            del channel[end:]
            for index in range(len(channel)):
                channel[index] *= gain
            for offset in range(min(fade_samples, len(channel))):
                channel[len(channel) - 1 - offset] *= offset / fade_samples

    def write(self, path):
        frames = bytearray()
        for left, right in zip(self.left, self.right):
            frames += struct.pack(
                "<hh",
                max(-32767, min(32767, round(left * 32767))),
                max(-32767, min(32767, round(right * 32767))),
            )
        with wave.open(path, "wb") as out:
            out.setnchannels(2)
            out.setsampwidth(2)
            out.setframerate(RATE)
            out.writeframes(bytes(frames))


def chime():
    """기본 알림음(차임): 맑은 두 음이 4도 올라간다"""
    track = Track(1.3)
    track.tone(0.0, note("G5"), GLASS, 0.26, pan=-0.15)
    track.tone(0.11, note("C6"), GLASS, 0.32, amp=0.92, pan=0.15)
    track.reverb(0.12, room=0.72)
    return track


def bubble():
    """물방울: 아래에서 위로 빠르게 미끄러지는 짧은 음 둘"""
    track = Track(0.8)
    track.tone(0.0, note("D6"), [(1.0, 1.0, 1.0), (2.0, 0.12, 0.5)], 0.075, sweep=(0.45, 0.018))
    track.tone(0.085, note("A6"), [(1.0, 0.7, 1.0), (2.0, 0.08, 0.5)], 0.06, sweep=(0.5, 0.015))
    track.reverb(0.1, room=0.68)
    return track


def bell():
    """종: 작은 종이 한 번 울린다"""
    track = Track(2.0)
    track.tone(0.0, note("A5"), BELL, 0.48, attack=0.003)
    track.reverb(0.12, room=0.74)
    return track


def rise():
    """오름(완료): 장3화음이 차례로 올라간다"""
    track = Track(1.4)
    for index, (name, pan) in enumerate((("C6", -0.25), ("E6", 0.0), ("G6", 0.25))):
        track.tone(index * 0.075, note(name), GLASS, 0.28, amp=1.0 - 0.08 * index, pan=pan)
    track.reverb(0.12, room=0.72)
    return track


def glass():
    """반짝(정보): 높고 맑게 한 번 친다"""
    track = Track(1.0)
    track.tone(0.0, note("C7"), GLASS, 0.17, attack=0.002)
    track.tone(0.0, note("C7") * 2.76, [(1.0, 0.18, 1.0)], 0.09, attack=0.002)
    track.reverb(0.11, room=0.7)
    return track


def knock():
    """둥(경고): 낮고 둥근 음이 조금 내려앉는다. 작은 스피커에서도 들리게 배음을 섞는다"""
    track = Track(0.9)
    partials = [(1.0, 1.0, 1.0), (2.0, 0.55, 0.6), (3.0, 0.3, 0.4), (4.2, 0.12, 0.25)]
    track.tone(0.0, note("A3"), partials, 0.16, attack=0.003, sweep=(1.18, 0.03))
    track.tone(0.0, note("A4"), [(1.0, 0.25, 1.0)], 0.1, attack=0.003)
    track.reverb(0.08, room=0.62)
    return track


def ping():
    """핑(주의): 부드럽게 시작해 오래 울리는 맑은 음"""
    track = Track(1.8)
    track.tone(0.0, note("D6"), [(1.0, 1.0, 1.0), (2.0, 0.1, 0.5), (3.0, 0.03, 0.3)], 0.36, attack=0.012)
    track.reverb(0.18, room=0.78)
    return track


def startup():
    """시동음: D♭장조 add9 화음을 아래에서 위로 퉁긴다. 낮은 음이 받치고 높은 음이 오래 반짝인다"""
    track = Track(3.8)
    voices = [
        ("Db3", 0.32, 1.05, -0.0),
        ("Db4", 0.62, 1.0, -0.3),
        ("Ab4", 0.6, 0.95, 0.25),
        ("Db5", 0.55, 0.9, -0.15),
        ("Eb5", 0.42, 0.85, 0.35),
        ("F5", 0.48, 0.82, -0.35),
        ("Ab5", 0.4, 0.78, 0.2),
        ("Db6", 0.22, 0.75, 0.0),
    ]
    for index, (name, amp, decay, pan) in enumerate(voices):
        track.tone(index * 0.028, note(name), WARM, decay, amp=amp, attack=0.022, pan=pan)
    track.reverb(0.2, room=0.82, damp=0.4)
    return track


# 소리 이름 → 만드는 함수
SOUNDS = {
    "message-new-instant": chime,
    "message": bubble,
    "bell": bell,
    "complete": rise,
    "dialog-information": glass,
    "dialog-warning": knock,
    "window-attention": ping,
    "system-ready": startup,
}

INDEX_THEME = """\
[Sound Theme]
Name=buntOS
Comment=buntOS sounds made by tools/sounds/generate.py
Inherits=freedesktop
Directories=stereo

[stereo]
OutputProfile=stereo
"""


def main(argv):
    if len(argv) < 2:
        print("usage: generate.py <output dir> [sound name...]", file=sys.stderr)
        return 2
    out = argv[1]
    names = argv[2:] or list(SOUNDS)
    unknown = [name for name in names if name not in SOUNDS]
    if unknown:
        print(f"unknown sounds: {', '.join(unknown)}", file=sys.stderr)
        return 2
    os.makedirs(os.path.join(out, "stereo"), exist_ok=True)
    with open(os.path.join(out, "index.theme"), "w", encoding="utf-8") as index:
        index.write(INDEX_THEME)
    for name in names:
        track = SOUNDS[name]()
        # 시동음은 길게 울리므로 끝을 더 천천히 줄인다
        track.finish(fade=0.35 if name == "system-ready" else 0.08)
        path = os.path.join(out, "stereo", f"{name}.wav")
        track.write(path)
        print(f"{path}: {len(track.left) / RATE:.2f} s, {track.loudness():.1f} LUFS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
