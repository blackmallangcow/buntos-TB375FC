# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""안드로이드 boot·init_boot(헤더 v3/v4), vendor_boot(v3/v4), dtbo 이미지를 푼다. 표준 라이브러리만 쓴다.
커널과 램디스크는 압축된 그대로 내고(MediaTek은 LZ4 legacy), DTB는 장치 트리마다 따로 낸다. vendor_boot의 DTB
영역은 FDT를 이어 붙인 꼴과 DT 표(dtbo와 같은 꼴, MT6897) 둘 다 받는다.

사용법: bootimg.py <boot.img> <vendor_boot.img> <init_boot.img> <dtbo.img> <출력 디렉터리>
"""
import os, struct, sys

def align(n, page):
    return (n + page - 1) // page * page

def boot(path, out, prefix):
    data = open(path, "rb").read()
    assert data[:8] == b"ANDROID!", path
    kernel_size, ramdisk_size, os_version, header_size = struct.unpack("<IIII", data[8:24])
    header_version = struct.unpack("<I", data[40:44])[0]
    cmdline = data[44:44 + 1536].split(b"\0")[0].decode(errors="replace")
    page = 4096
    kernel = data[page:page + kernel_size]
    roff = page + align(kernel_size, page)
    ramdisk = data[roff:roff + ramdisk_size]
    ver = os_version >> 11
    print(f"{prefix}: header v{header_version}, kernel {kernel_size}, ramdisk {ramdisk_size}, "
          f"os {(ver >> 14) & 0x7f}.{(ver >> 7) & 0x7f}.{ver & 0x7f} patch {2000 + ((os_version >> 4) & 0x7f)}-{os_version & 0xf:02d}, cmdline {cmdline!r}")
    if kernel:
        open(os.path.join(out, f"{prefix}.kernel"), "wb").write(kernel)
    if ramdisk:
        open(os.path.join(out, f"{prefix}.ramdisk"), "wb").write(ramdisk)

def vendor_boot(path, out):
    data = open(path, "rb").read()
    assert data[:8] == b"VNDRBOOT", path
    version, page, kaddr, raddr, vr_size = struct.unpack("<IIIII", data[8:28])
    cmdline = data[28:28 + 2048].split(b"\0")[0].decode(errors="replace")
    tags_addr = struct.unpack("<I", data[2076:2080])[0]
    name = data[2080:2096].split(b"\0")[0].decode(errors="replace")
    header_size, dtb_size = struct.unpack("<II", data[2096:2104])
    dtb_addr = struct.unpack("<Q", data[2104:2112])[0]
    table_size = entry_num = entry_size = bootconfig_size = 0
    if version >= 4:
        table_size, entry_num, entry_size, bootconfig_size = struct.unpack("<IIII", data[2112:2128])
    print(f"vendor_boot: v{version}, page {page}, name {name!r}, vendor ramdisk {vr_size}, dtb {dtb_size}, "
          f"ramdisk table {entry_num} entries, bootconfig {bootconfig_size}")
    print(f"vendor_boot cmdline: {cmdline!r}")
    pos = align(header_size, page)
    vr = data[pos:pos + vr_size]; pos += align(vr_size, page)
    dtb = data[pos:pos + dtb_size]; pos += align(dtb_size, page)
    table = data[pos:pos + table_size]; pos += align(table_size, page)
    bootconfig = data[pos:pos + bootconfig_size]
    open(os.path.join(out, "vendor_boot.dtb"), "wb").write(dtb)
    if bootconfig:
        open(os.path.join(out, "vendor_boot.bootconfig"), "wb").write(bootconfig)
    for index in range(entry_num):
        e = table[index * entry_size:(index + 1) * entry_size]
        rsize, roff, rtype = struct.unpack("<III", e[:12])
        rname = e[12:44].split(b"\0")[0].decode(errors="replace")
        print(f"  vendor ramdisk {index}: type {rtype}, name {rname!r}, size {rsize}")
        open(os.path.join(out, f"vendor_ramdisk{index}.cpio.compressed"), "wb").write(vr[roff:roff + rsize])
    # DT 표(MT6897)면 표의 항목마다, 아니면 이어 붙은 DTB를 FDT 머리(0xd00dfeed)마다 나눈다
    if dtb[:4] == b"\xd7\xb7\xab\x1e":
        dt_table(dtb, out, "vendor_boot")
        return
    off, n = 0, 0
    while off + 8 <= len(dtb):
        magic, total = struct.unpack(">II", dtb[off:off + 8])
        if magic != 0xD00DFEED:
            break
        open(os.path.join(out, f"vendor_boot-{n}.dtb"), "wb").write(dtb[off:off + total])
        print(f"  dtb {n}: {total} bytes")
        off += total; n += 1

def dt_table(data, out, prefix):
    """DT 표(magic 0xd7b7ab1e, 큰 끝 정수)의 항목마다 DTB를 낸다"""
    magic, total, hsize, esize, count, eoff, page, version = struct.unpack(">8I", data[:32])
    assert magic == 0xD7B7AB1E, hex(magic)
    print(f"{prefix}: DT table v{version}, {count} entries")
    for i in range(count):
        e = data[eoff + i * esize:eoff + (i + 1) * esize]
        size, off, ident, rev = struct.unpack(">4I", e[:16])
        custom = struct.unpack(">4I", e[16:32])
        open(os.path.join(out, f"{prefix}-{i}.dtb"), "wb").write(data[off:off + size])
        print(f"  entry {i}: {size} bytes, id {ident:#x}, rev {rev:#x}, custom {[hex(c) for c in custom]}")

def dtbo(path, out):
    dt_table(open(path, "rb").read(), out, "dtbo")

def main(argv):
    boot_img, vendor_img, init_img, dtbo_img, out = argv[1:6]
    os.makedirs(out, exist_ok=True)
    boot(boot_img, out, "boot")
    boot(init_img, out, "init_boot")
    vendor_boot(vendor_img, out)
    dtbo(dtbo_img, out)

if __name__ == "__main__":
    main(sys.argv)
