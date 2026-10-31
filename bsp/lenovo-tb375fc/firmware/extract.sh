#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 기기에서 받아 둔 원천(파티션 백업, vendor·nvdata 파일)에서 manifest.tsv의 펌웨어와 보정값을 골라 모은다.
# 원천을 읽기만 하고 기기에는 닿지 않는다. 출력은 저장소 밖에 둔다(레노버·MediaTek 파일, 기기마다 다른 보정값).
# 뼈대다. 기기를 받은 뒤 manifest의 이름과 자리를 확인하고, 드라이버가 정해지면 목적지(지금은 대부분 -)를 채운다.
#
# 사용법: extract.sh <원천 디렉터리> <출력 디렉터리>
#   원천  partitions/<이름>.bin  파티션 백업(GPT 파티션 이름, A/B면 _a·_b가 붙는다)
#         vendor/                vendor 파티션의 파일 (pull-android.sh가 받은 /vendor/firmware 등)
#         nvdata/                nvdata 파일 시스템의 파일. 없고 partitions/nvdata.bin이 있으면 debugfs로 그 안에서 꺼낸다
#   출력  고른 파일, unsorted/(목적지를 아직 정하지 않은 것), SHA256SUMS, report.txt(찾음·없음)
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

die() {
	echo "extract: $*" >&2
	exit 1
}

src=${1:?"원천 디렉터리가 필요하다 (사용법은 이 파일의 머리말)"}
out=${2:?"출력 디렉터리가 필요하다"}
[[ -d $src ]] || die "$src가 없다"
src=$(cd "$src" && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
case $out/ in "$src"/*) die "출력을 원천 안에 두지 않는다" ;; esac

# 원천을 고치지 않으려고 작업 디렉터리에 보기를 만든다 (nvdata/만 백업 이미지에서 꺼낸 것으로 갈음할 수 있다)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
view="$work/view"
mkdir -p "$view"
for d in partitions vendor nvdata; do
	[[ -d $src/$d ]] && ln -s "$src/$d" "$view/$d"
done
if [[ ! -e $view/nvdata ]]; then
	img=
	for cand in "$src/partitions/nvdata.bin" "$src/partitions/nvdata_a.bin"; do
		[[ -f $cand ]] && img=$cand && break
	done
	if [[ -n $img ]] && command -v debugfs >/dev/null; then
		mkdir -p "$view/nvdata"
		# -c: 읽기 전용(비트맵을 읽지 않는다). root가 아니면 소유권을 못 바꾼다는 경고가 난다
		debugfs -c -R "rdump / $view/nvdata" "$img" >/dev/null 2>&1 || true
		echo "nvdata: ${img#"$src"/}에서 꺼냈다"
	fi
fi

found=0 missing=0
: >"$out/report.txt"
while IFS=$'\t' read -r kind source dest user status; do
	[[ -z $kind || $kind == \#* ]] && continue
	files=()
	case $kind in
	partition)
		for cand in "$view/partitions/$source.bin" "$view/partitions/${source}_a.bin"; do
			if [[ -f $cand ]]; then
				files+=("$cand")
				break
			fi
		done
		;;
	file)
		# 원천 칸의 글롭을 펼친다 (맞는 것이 없으면 글자 그대로 남아 -f에 걸린다)
		for f in "$view"/$source; do
			[[ -f $f ]] && files+=("$f")
		done
		;;
	*) die "manifest의 모르는 종류: $kind" ;;
	esac
	if ((${#files[@]} == 0)); then
		echo "없음	$kind	$source	$status" >>"$out/report.txt"
		missing=$((missing + 1))
		continue
	fi
	for f in "${files[@]}"; do
		rel=${f#"$view"/}
		if [[ $dest == - ]]; then
			target="unsorted/$rel"
		else
			target=$dest
		fi
		install -D -m 0644 "$f" "$out/$target"
		echo "찾음	$kind	$rel	-> $target	$user" >>"$out/report.txt"
		found=$((found + 1))
	done
done <"$here/manifest.tsv"

(cd "$out" && find . -type f ! -name SHA256SUMS ! -name report.txt -print0 | sort -z | xargs -0 -r sha256sum) \
	>"$out/SHA256SUMS"
cat "$out/report.txt"
echo "찾음 $found, 없음 $missing: $out"
