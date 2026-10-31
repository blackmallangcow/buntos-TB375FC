# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""mkimages.sh가 만든 이미지 넷의 머리 필드를 원래 펌웨어(글로벌판 ZUI 17.0.04.266)와 견준다. 표준 라이브러리만 쓴다.
원래 이미지 디렉터리를 주면 그 값과, 주지 않으면 아래 REFERENCE(그 펌웨어에서 읽은 값)와 견준다. 원래 이미지를 주면
그 값이 REFERENCE와 같은지도 본다(다른 판의 펌웨어인지). 크기·명령줄처럼 달라야 하는 필드는 "다름(예상)"으로 적고,
그 밖의 필드가 다르면 끝 상태가 1이다.

사용법: check_images.py <우리 이미지 디렉터리> [<원래 이미지 디렉터리>]
"""
import os, struct, sys

LZ4_LEGACY = "02214c18"
REFERENCE = {
    "boot": {"magic": "ANDROID!", "header_version": 4, "header_size": 1584, "os_version": 0, "reserved": (0, 0, 0, 0),
             "signature_size": 0, "kernel_format": LZ4_LEGACY, "ramdisk_size": 0, "cmdline": ""},
    "init_boot": {"magic": "ANDROID!", "header_version": 4, "header_size": 1584, "os_version": 0,
                  "reserved": (0, 0, 0, 0), "signature_size": 0, "kernel_size": 0, "ramdisk_format": LZ4_LEGACY,
                  "cmdline": ""},
    "vendor_boot": {"magic": "VNDRBOOT", "header_version": 4, "page_size": 4096, "kernel_addr": 0x40000000,
                    "ramdisk_addr": 0x66F00000, "tags_addr": 0x47C80000, "dtb_addr": 0x47C80000, "name": "",
                    "header_size": 2128, "cmdline": "bootopt=64S3,32N2,64N2", "table_entry_num": 2,
                    "table_entry_size": 108, "bootconfig_size": 0,
                    "ramdisk_types": (1, 2), "ramdisk_names": ("", "recovery"), "board_ids_zero": True,
                    "ramdisk_formats": (LZ4_LEGACY, LZ4_LEGACY), "dt.magic": 0xD7B7AB1E, "dt.header_size": 0x20,
                    "dt.entry_size": 0x20, "dt.entries_offset": 0x20, "dt.page_size": 0x800, "dt.version": 0,
                    "dt.entry_count": 1, "dt.first_offset": 0x40, "dt.ids": ((0, 0, 0, 0, 0, 0),), "dt.fdt_magic": True},
    "dtbo": {"dt.magic": 0xD7B7AB1E, "dt.header_size": 0x20, "dt.entry_size": 0x20, "dt.entries_offset": 0x20,
             "dt.page_size": 0x800, "dt.version": 0, "dt.entry_count": 1, "dt.first_offset": 0x40,
             "dt.ids": ((0, 0, 0, 0, 0, 0),), "dt.fdt_magic": True},
}
# 달라도 되는 필드와 까닭
EXPECTED = {
    "boot": {"kernel_size": "우리 커널", "cmdline": "우리 명령줄(원래는 비어 있다)", "ramdisk_size": "--boot-ramdisk",
             "avb_footer": "AVB 꼬리를 달지 않는다", "file_size": "파티션 크기로 채우지 않는다"},
    "init_boot": {"ramdisk_size": "우리 initramfs", "avb_footer": "AVB 꼬리를 달지 않는다",
                  "file_size": "파티션 크기로 채우지 않는다"},
    "vendor_boot": {"vendor_ramdisk_size": "우리 벤더 램디스크(표지만)", "ramdisk_sizes": "우리 벤더 램디스크(표지만)",
                    "ramdisk_offsets": "우리 벤더 램디스크(표지만)", "dtb_size": "우리 장치 트리",
                    "dt.total_size": "우리 장치 트리", "avb_footer": "AVB 꼬리를 달지 않는다",
                    "file_size": "파티션 크기로 채우지 않는다"},
    "dtbo": {"dt.total_size": "빈 오버레이", "trailing": "MediaTek 서명 블록(cert1·cert2)을 붙이지 않는다",
             "avb_footer": "AVB 꼬리가 원래도 없다", "file_size": "빈 오버레이"},
}

def align(n, page):
    return (n + page - 1) // page * page

def footer(data):
    return data[-64:-60] == b"AVBf"

def dt_table(data, prefix="dt."):
    magic, total, hsize, esize, count, eoff, page, version = struct.unpack(">8I", data[:32])
    f = {prefix + "magic": magic, prefix + "total_size": total, prefix + "header_size": hsize,
         prefix + "entry_size": esize, prefix + "entries_offset": eoff, prefix + "page_size": page,
         prefix + "version": version, prefix + "entry_count": count}
    entries = [struct.unpack(">8I", data[eoff + i * esize:eoff + i * esize + 32]) for i in range(count)]
    f[prefix + "first_offset"] = entries[0][1] if entries else None
    f[prefix + "ids"] = tuple(e[2:] for e in entries)
    f[prefix + "fdt_magic"] = all(data[e[1]:e[1] + 4] == b"\xd0\x0d\xfe\xed" for e in entries)
    return f, total

def boot(data):
    magic, ksize, rsize, osv, hsize = struct.unpack("<8sIIII", data[:24])
    reserved = struct.unpack("<4I", data[24:40])
    version = struct.unpack("<I", data[40:44])[0]
    f = {"magic": magic.decode(errors="replace"), "header_version": version, "header_size": hsize, "os_version": osv,
         "reserved": reserved, "signature_size": struct.unpack("<I", data[1580:1584])[0], "kernel_size": ksize,
         "ramdisk_size": rsize, "cmdline": data[44:44 + 1536].split(b"\0")[0].decode(errors="replace")}
    if ksize:
        f["kernel_format"] = data[4096:4100].hex()
    if rsize:
        roff = 4096 + align(ksize, 4096)
        f["ramdisk_format"] = data[roff:roff + 4].hex()
    return f

def vendor_boot(data):
    magic, version, page, kaddr, raddr, vrsize = struct.unpack("<8sIIIII", data[:28])
    f = {"magic": magic.decode(errors="replace"), "header_version": version, "page_size": page, "kernel_addr": kaddr,
         "ramdisk_addr": raddr, "vendor_ramdisk_size": vrsize,
         "cmdline": data[28:28 + 2048].split(b"\0")[0].decode(errors="replace")}
    f["tags_addr"] = struct.unpack("<I", data[2076:2080])[0]
    f["name"] = data[2080:2096].split(b"\0")[0].decode(errors="replace")
    f["header_size"], f["dtb_size"] = struct.unpack("<II", data[2096:2104])
    f["dtb_addr"] = struct.unpack("<Q", data[2104:2112])[0]
    tsize, tnum, tesize, bcsize = struct.unpack("<IIII", data[2112:2128])
    f.update({"table_entry_num": tnum, "table_entry_size": tesize, "bootconfig_size": bcsize})
    pos = align(f["header_size"], page)
    vr = data[pos:pos + vrsize]
    pos += align(vrsize, page)
    dtb = data[pos:pos + f["dtb_size"]]
    pos += align(f["dtb_size"], page)
    table = data[pos:pos + tsize]
    entries = []
    for i in range(tnum):
        e = table[i * tesize:(i + 1) * tesize]
        rsize, roff, rtype = struct.unpack("<III", e[:12])
        entries.append((rsize, roff, rtype, e[12:44].split(b"\0")[0].decode(errors="replace"), struct.unpack("<16I", e[44:108])))
    f["ramdisk_sizes"] = tuple(e[0] for e in entries)
    f["ramdisk_offsets"] = tuple(e[1] for e in entries)
    f["ramdisk_types"] = tuple(e[2] for e in entries)
    f["ramdisk_names"] = tuple(e[3] for e in entries)
    f["board_ids_zero"] = all(not any(e[4]) for e in entries)
    f["ramdisk_formats"] = tuple(vr[e[1]:e[1] + 4].hex() for e in entries)
    dt, _ = dt_table(dtb)
    f.update(dt)
    return f

def dtbo(data):
    f, total = dt_table(data)
    f["trailing"] = any(data[total:])
    return f

def read(directory, name):
    data = open(os.path.join(directory, name + ".img"), "rb").read()
    f = {"boot": boot, "init_boot": boot, "vendor_boot": vendor_boot, "dtbo": dtbo}[name](data)
    f["avb_footer"] = footer(data)
    f["file_size"] = len(data)
    return f

def show(v):
    if isinstance(v, int) and not isinstance(v, bool) and v > 0xFFFF:
        return hex(v)
    return repr(v) if isinstance(v, str) else str(v)

def main(argv):
    if len(argv) not in (2, 3):
        raise SystemExit(__doc__)
    ours_dir = argv[1]
    stock_dir = argv[2] if len(argv) == 3 else None
    bad = 0
    for name in ("boot", "init_boot", "vendor_boot", "dtbo"):
        ours = read(ours_dir, name)
        ref = REFERENCE[name]
        if stock_dir:
            stock = read(stock_dir, name)
            for key, want in ref.items():
                if stock.get(key) != want:
                    print(f"주의: 원래 {name}.img의 {key}={show(stock.get(key))}가 REFERENCE({show(want)})와 다르다")
                    bad += 1
        else:
            stock = ref
        print(f"{name}.img ({'원래 이미지' if stock_dir else 'REFERENCE'}와 견줌)")
        for key in sorted(set(ours) | set(stock), key=lambda k: (k not in ref, k)):
            a, b = ours.get(key), stock.get(key, "-")
            if b == "-" and key not in ref:
                verdict = "참고"
            elif a == b:
                verdict = "같음"
            elif key in EXPECTED[name]:
                verdict = f"다름(예상: {EXPECTED[name][key]})"
            else:
                verdict = "다름"
                bad += 1
            print(f"  {key:22} {show(a):34.34} {show(b):34.34} {verdict}")
    print("판정:", "머리 필드가 원래 꼴과 맞다" if not bad else f"뜻밖의 차이 {bad}개")
    return 1 if bad else 0

if __name__ == "__main__":
    sys.exit(main(sys.argv))
