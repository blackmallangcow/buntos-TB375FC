# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""원격 ZIP의 목차를 읽고 고른 항목만 받아 푼다(HTTP Range). 펌웨어 묶음(수 GB)에서 몇 MB짜리 이미지만 받을 때
쓴다(docs/hw/lenovo-tb375fc.md). 표준 라이브러리만 쓴다. ZIP64를 읽는다.

사용법: zipremote.py list <URL>
        zipremote.py get <URL> <항목 이름> <출력 파일>   (CRC32를 견준다)
"""
import struct, sys, urllib.request, zlib

UA = {"User-Agent": "Mozilla/5.0 (buntOS tools/hw/zipremote.py)"}

def fetch(url, start, end):
    req = urllib.request.Request(url, headers={**UA, "Range": f"bytes={start}-{end}"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        if resp.status != 206:
            raise SystemExit(f"range not honored: HTTP {resp.status}")
        return resp.read()

def size_of(url):
    req = urllib.request.Request(url, method="HEAD", headers=UA)
    with urllib.request.urlopen(req, timeout=60) as resp:
        return int(resp.headers["Content-Length"])

def central_directory(url):
    total = size_of(url)
    tail_len = min(total, 128 * 1024)
    tail = fetch(url, total - tail_len, total - 1)
    eocd = tail.rfind(b"PK\x05\x06")
    if eocd < 0:
        raise SystemExit("no end of central directory")
    (_, _, _, _, count, cd_size, cd_offset, _) = struct.unpack("<IHHHHIIH", tail[eocd:eocd + 22])
    loc = tail.rfind(b"PK\x06\x07", 0, eocd)
    if loc >= 0:  # ZIP64
        (_, _, z64_offset, _) = struct.unpack("<IIQI", tail[loc:loc + 20])
        rec = fetch(url, z64_offset, z64_offset + 56 - 1)
        (sig, _, _, _, _, _, _, count, cd_size, cd_offset) = struct.unpack("<IQHHIIQQQQ", rec[:56])
        assert sig == 0x06064B50
    cd = fetch(url, cd_offset, cd_offset + cd_size - 1)
    entries, pos = [], 0
    for _ in range(count):
        (sig, _, _, flags, method, _, _, crc, csize, usize, nlen, xlen, clen, _, _, _, offset) = struct.unpack(
            "<IHHHHHHIIIHHHHHII", cd[pos:pos + 46])
        assert sig == 0x02014B50, hex(sig)
        name = cd[pos + 46:pos + 46 + nlen].decode("utf-8", "replace")
        extra = cd[pos + 46 + nlen:pos + 46 + nlen + xlen]
        # ZIP64 확장 필드: 0xFFFFFFFF인 값만 차례로 들어 있다
        xpos = 0
        while xpos + 4 <= len(extra):
            hid, hsize = struct.unpack("<HH", extra[xpos:xpos + 4])
            body = extra[xpos + 4:xpos + 4 + hsize]
            if hid == 1:
                vals, bpos = [], 0
                while bpos + 8 <= len(body):
                    vals.append(struct.unpack("<Q", body[bpos:bpos + 8])[0]); bpos += 8
                it = iter(vals)
                if usize == 0xFFFFFFFF: usize = next(it)
                if csize == 0xFFFFFFFF: csize = next(it)
                if offset == 0xFFFFFFFF: offset = next(it)
            xpos += 4 + hsize
        entries.append({"name": name, "method": method, "csize": csize, "usize": usize, "crc": crc,
                        "offset": offset, "flags": flags})
        pos += 46 + nlen + xlen + clen
    return total, entries

def main(argv):
    cmd, url = argv[1], argv[2]
    total, entries = central_directory(url)
    if cmd == "list":
        print(f"archive {total} bytes, {len(entries)} entries")
        for e in sorted(entries, key=lambda e: e["offset"]):
            print(f"{e['csize']:>12} {e['usize']:>12} m{e['method']} {e['name']}")
        return 0
    name, out = argv[3], argv[4]
    entry = next((e for e in entries if e["name"] == name), None)
    if entry is None:
        raise SystemExit(f"no entry {name}")
    head = fetch(url, entry["offset"], entry["offset"] + 30 - 1)
    (sig, _, _, _, _, _, _, _, _, nlen, xlen) = struct.unpack("<IHHHHHIIIHH", head)
    assert sig == 0x04034B50
    start = entry["offset"] + 30 + nlen + xlen
    end = start + entry["csize"] - 1
    dec = zlib.decompressobj(-15) if entry["method"] == 8 else None
    if entry["method"] not in (0, 8):
        raise SystemExit(f"unsupported method {entry['method']}")
    crc, written, chunk = 0, 0, 16 * 1024 * 1024
    with open(out, "wb") as f:
        pos = start
        while pos <= end:
            data = fetch(url, pos, min(end, pos + chunk - 1))
            pos += len(data)
            data = dec.decompress(data) if dec else data
            crc = zlib.crc32(data, crc); written += len(data); f.write(data)
        if dec:
            data = dec.flush(); crc = zlib.crc32(data, crc); written += len(data); f.write(data)
    ok = (crc & 0xFFFFFFFF) == entry["crc"] and written == entry["usize"]
    print(f"{name}: {entry['csize']} -> {written} bytes, crc {'ok' if ok else 'MISMATCH'}")
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main(sys.argv))
