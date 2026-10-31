# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""DT 표(안드로이드 dtbo 꼴, magic 0xd7b7ab1e, 큰 끝 정수)를 만든다. 표준 라이브러리만 쓴다.
MT6897의 LK는 vendor_boot의 DTB 영역과 dtbo 파티션에서 이 꼴을 읽는다. 원래 펌웨어(ZUI 17.0.04.266)와 같게
머리 0x20바이트, 항목 0x20바이트, 항목은 머리 바로 뒤, 쪽 크기 0x800, 판 0이고 항목의 id·rev·custom은 0이다.
첫 DTB는 항목 표 바로 뒤(항목 하나면 0x40)에 오고, 뒤의 DTB는 8바이트 경계에 둔다(libfdt가 맞춤을 검사한다).
푸는 쪽은 tools/hw/bootimg.py다.

사용법: dt_table.py create <출력> <DTB>...   DTB(오버레이 DTBO도 된다)마다 항목 하나
        dt_table.py dump <DT 표>             머리와 항목을 보인다
"""
import struct, sys

MAGIC = 0xD7B7AB1E
HEADER_SIZE = ENTRY_SIZE = 0x20
PAGE_SIZE = 0x800

def create(out, paths):
    blobs = []
    for path in paths:
        data = open(path, "rb").read()
        if len(data) < 40 or struct.unpack(">I", data[:4])[0] != 0xD00DFEED:
            raise SystemExit(f"{path}: FDT가 아니다")
        total = struct.unpack(">I", data[4:8])[0]
        if total != len(data):
            raise SystemExit(f"{path}: FDT totalsize {total}가 파일 크기 {len(data)}와 다르다")
        blobs.append(data)
    if not blobs:
        raise SystemExit("DTB가 하나는 있어야 한다")
    entries, body = [], b""
    offset = HEADER_SIZE + ENTRY_SIZE * len(blobs)
    for data in blobs:
        pad = -offset % 8 if body else 0
        body += b"\0" * pad
        offset += pad
        entries.append(struct.pack(">8I", len(data), offset, 0, 0, 0, 0, 0, 0))
        body += data
        offset += len(data)
    header = struct.pack(">8I", MAGIC, offset, HEADER_SIZE, ENTRY_SIZE, len(blobs), HEADER_SIZE, PAGE_SIZE, 0)
    with open(out, "wb") as f:
        f.write(header + b"".join(entries) + body)

def dump(path):
    data = open(path, "rb").read()
    magic, total, hsize, esize, count, eoff, page, version = struct.unpack(">8I", data[:32])
    if magic != MAGIC:
        raise SystemExit(f"{path}: DT 표가 아니다 (magic {magic:#x})")
    print(f"total_size {total:#x} ({total}), header_size {hsize:#x}, dt_entry_size {esize:#x}, dt_entry_count {count}, "
          f"dt_entries_offset {eoff:#x}, page_size {page:#x}, version {version}, file {len(data)}")
    for i in range(count):
        size, off, ident, rev, *custom = struct.unpack(">8I", data[eoff + i * esize:eoff + i * esize + 32])
        fdt = struct.unpack(">I", data[off:off + 4])[0] if off + 4 <= len(data) else 0
        print(f"  entry {i}: dt_size {size}, dt_offset {off:#x}, id {ident:#x}, rev {rev:#x}, "
              f"custom {[hex(c) for c in custom]}, magic {fdt:#x}")

def main(argv):
    if len(argv) >= 4 and argv[1] == "create":
        create(argv[2], argv[3:])
    elif len(argv) == 3 and argv[1] == "dump":
        dump(argv[2])
    else:
        raise SystemExit(__doc__)

if __name__ == "__main__":
    main(sys.argv)
