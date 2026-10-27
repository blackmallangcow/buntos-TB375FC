# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""buntOS 기본 배경화면을 만든다. 표준 라이브러리만 쓴다 (빌드 VM에 numpy가 없다).

사용법: python3 -I generate.py <출력 디렉터리>
그림마다 세로 그라데이션 위에 큰 빛 번짐 몇 개를 겹치고, 띠가 보이지 않게 순서 디더링을 한다.
tools/image/build.sh가 이 파일이 바뀌었을 때만 다시 만들어 이미지의
/usr/share/backgrounds/buntos/에 넣는다.
"""

import math
import os
import struct
import sys
import zlib

WIDTH, HEIGHT = 1920, 1200

# 이름: (위 색, 아래 색, [(가로 위치, 세로 위치, 반지름, 색, 세기)...]) — 위치와 반지름은 화면 비율
WALLPAPERS = {
    # 기본: 컴포지터 그라데이션(comp/data/comp.toml)과 같은 파랑→분홍
    "buntos-dawn": (
        (0x5B, 0x7C, 0xFA),
        (0xF2, 0xA7, 0xC3),
        [
            (0.18, 0.30, 0.55, (0x8E, 0x6C, 0xF5), 0.55),
            (0.85, 0.75, 0.50, (0xFF, 0xC4, 0x9B), 0.45),
            (0.65, 0.10, 0.35, (0x4F, 0xC3, 0xF7), 0.35),
        ],
    ),
    "buntos-ocean": (
        (0x06, 0x2A, 0x5C),
        (0x0B, 0x8F, 0xA8),
        [
            (0.25, 0.75, 0.55, (0x14, 0xC8, 0xB8), 0.45),
            (0.80, 0.20, 0.45, (0x3D, 0x5A, 0xFE), 0.50),
        ],
    ),
    "buntos-sunset": (
        (0x3B, 0x1E, 0x6E),
        (0xFF, 0x9E, 0x5E),
        [
            (0.70, 0.80, 0.55, (0xFF, 0x5E, 0x7A), 0.55),
            (0.20, 0.20, 0.45, (0x6A, 0x3F, 0xD8), 0.45),
        ],
    ),
    "buntos-forest": (
        (0x0F, 0x3D, 0x2E),
        (0x9C, 0xD6, 0x8C),
        [
            (0.75, 0.30, 0.50, (0x2E, 0xB8, 0x86), 0.45),
            (0.20, 0.85, 0.45, (0xE8, 0xE2, 0x8C), 0.35),
        ],
    ),
    "buntos-graphite": (
        (0x1C, 0x1C, 0x22),
        (0x3A, 0x3A, 0x48),
        [
            (0.30, 0.25, 0.55, (0x4A, 0x4F, 0x7A), 0.40),
            (0.80, 0.80, 0.45, (0x5A, 0x3E, 0x6E), 0.35),
        ],
    ),
}

# 4x4 순서 디더링 (−0.5 ~ +0.5)
BAYER = [
    [(v + 0.5) / 16 - 0.5 for v in row]
    for row in ([0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5])
]


def render(top, bottom, glows):
    # 빛 번짐은 가우스 모양이라 가로·세로로 나뉜다: exp(-(dx²+dy²)/r²) = exp(-dx²/r²)·exp(-dy²/r²)
    scale = max(WIDTH, HEIGHT)
    factors = []
    for gx, gy, radius, color, strength in glows:
        r = radius * scale
        fx = [math.exp(-(((x - gx * WIDTH) / r) ** 2)) for x in range(WIDTH)]
        fy = [math.exp(-(((y - gy * HEIGHT) / r) ** 2)) * strength for y in range(HEIGHT)]
        factors.append((fx, fy, color))

    rows = []
    for y in range(HEIGHT):
        t = y / (HEIGHT - 1)
        base = [top[c] + (bottom[c] - top[c]) * t for c in range(3)]
        dither = BAYER[y % 4]
        row = bytearray(1 + WIDTH * 3)
        for x in range(WIDTH):
            r, g, b = base
            for fx, fy, color in factors:
                a = fx[x] * fy[y]
                r += (color[0] - r) * a
                g += (color[1] - g) * a
                b += (color[2] - b) * a
            d = dither[x % 4]
            i = 1 + x * 3
            row[i] = min(255, max(0, int(r + d + 0.5)))
            row[i + 1] = min(255, max(0, int(g + d + 0.5)))
            row[i + 2] = min(255, max(0, int(b + d + 0.5)))
        rows.append(bytes(row))
    return b"".join(rows)


def png(path, raw):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    with open(path, "wb") as out:
        out.write(b"\x89PNG\r\n\x1a\n")
        out.write(chunk(b"IHDR", header))
        out.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        out.write(chunk(b"IEND", b""))


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, (top, bottom, glows) in WALLPAPERS.items():
        png(os.path.join(out, name + ".png"), render(top, bottom, glows))
        print(name)


if __name__ == "__main__":
    main()
