# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""디렉터리를 newc cpio(커널 initramfs 꼴)로 묶는다. 표준 라이브러리만 쓴다. 커널의 usr/gen_init_cpio처럼
장치 노드를 root 권한 없이 넣으려고 쓴다. initramfs에는 devtmpfs가 저절로 붙지 않으므로 커널이 init을 띄우기 전에
여는 /dev/console이 cpio 안에 있어야 한다.
모든 항목의 주인은 0:0, 시각은 SOURCE_DATE_EPOCH(없으면 0)이고, 이름 순서(디렉터리가 먼저)로 적어
같은 입력이면 같은 바이트가 나온다. 하드 링크는 따로 된 파일로 적는다.

사용법: mkcpio.py <출력> <디렉터리> [장치 노드]...
  장치 노드: <경로>:<모드 8진수>:c|b:<major>:<minor>   예: dev/console:600:c:5:1
"""
import os, stat, sys

def entry(ino, mode, nlink, mtime, name, data=b"", rdev=(0, 0)):
    name = name.encode() + b"\0"
    head = b"070701" + b"".join(b"%08X" % v for v in (
        ino, mode, 0, 0, nlink, mtime, len(data), 0, 0, rdev[0], rdev[1], len(name), 0))
    out = head + name
    out += b"\0" * (-len(out) % 4) + data
    return out + b"\0" * (-len(out) % 4)

def main(argv):
    if len(argv) < 3:
        raise SystemExit(__doc__)
    out, root = argv[1], argv[2]
    mtime = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    items = {}
    for top, dirs, files in os.walk(root):
        dirs.sort()
        for name in dirs + files:
            path = os.path.join(top, name)
            items[os.path.relpath(path, root)] = ("path", path)
    for spec in argv[3:]:
        name, mode, kind, major, minor = spec.split(":")
        if kind not in ("c", "b"):
            raise SystemExit(f"장치 종류는 c나 b다: {spec}")
        if os.path.dirname(name) and os.path.dirname(name) not in items:
            raise SystemExit(f"{name}의 디렉터리가 {root}에 없다")
        bits = stat.S_IFCHR if kind == "c" else stat.S_IFBLK
        items[name] = ("node", bits | int(mode, 8), int(major), int(minor))
    blob = []
    # 경로를 '/'로 나눠 견주면 디렉터리가 그 안의 항목보다 먼저 온다
    for ino, name in enumerate(sorted(items, key=lambda n: n.split("/")), 721):
        item = items[name]
        if item[0] == "node":
            blob.append(entry(ino, item[1], 1, mtime, name, rdev=(item[2], item[3])))
            continue
        st = os.lstat(item[1])
        perm = stat.S_IMODE(st.st_mode)
        if stat.S_ISLNK(st.st_mode):
            blob.append(entry(ino, stat.S_IFLNK | 0o777, 1, mtime, name, os.readlink(item[1]).encode()))
        elif stat.S_ISDIR(st.st_mode):
            blob.append(entry(ino, stat.S_IFDIR | perm, 2, mtime, name))
        elif stat.S_ISREG(st.st_mode):
            blob.append(entry(ino, stat.S_IFREG | perm, 1, mtime, name, open(item[1], "rb").read()))
        else:
            raise SystemExit(f"{item[1]}: 디렉터리, 파일, 심볼릭 링크만 묶는다(장치 노드는 인자로 준다)")
    blob.append(entry(0, 0, 1, 0, "TRAILER!!!"))
    with open(out, "wb") as f:
        f.write(b"".join(blob))

if __name__ == "__main__":
    main(sys.argv)
