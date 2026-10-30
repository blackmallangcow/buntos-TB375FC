# shellcheck shell=bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 이미지와 같은 Debian 스냅숏(image/mkosi.conf의 Snapshot=)에서 꾸러미를 깐 컨테이너 이미지를 docker로 만든다.
# tools/image/cargo-debian.sh(자체 컴포넌트의 빌드 환경)와 tools/image/tools-tree.sh(mkosi의 도구 트리)가 읽는다.
#
# debian_snapshot_image <바탕 이미지> <판> <apt 캐시 디렉터리> <저장소> <꾸러미...>
#   <저장소>는 줄마다 "아카이브 스위트..."다. 예: "debian trixie trixie-updates"와 "debian-security trixie-security"
#   바탕 이미지의 판(코드네임)이 <판>과 다르면 실패한다. 바탕 이미지에 이미 든 꾸러미도 스냅숏의 판으로 맞춘다.
#   만든 이미지의 이름을 출력한다. 바탕, 스냅숏, 저장소, 꾸러미와 이 파일이 같으면 이미 만든 이미지를 쓴다.
#   내려받은 꾸러미는 <apt 캐시 디렉터리>에 이 컴퓨터의 사용자 소유로 남는다 (CI가 캐시한다)

debian_snapshot_lib=${BASH_SOURCE[0]}

debian_snapshot_image() {
	local base=$1 release=$2 archives=$3 repositories=$4
	shift 4
	local repo snapshot image name
	repo=$(cd "$(dirname "$debian_snapshot_lib")/../.." && pwd)
	snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")
	if [[ ! "$snapshot" =~ ^[0-9]{8}T[0-9]{6}Z$ ]]; then
		echo "image/mkosi.conf의 Snapshot=을 읽지 못했다" >&2
		return 1
	fi
	image="buntos-debian-snapshot:$({
		printf '%s\n' "$base" "$release" "$snapshot" "$repositories" "$@"
		cat "$debian_snapshot_lib"
	} | sha256sum | cut -c1-16)"
	if docker image inspect "$image" >/dev/null 2>&1; then
		echo "$image"
		return
	fi
	mkdir -p "$archives"

	# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
	local prepare='
set -euo pipefail
. /etc/os-release
if [[ "${VERSION_CODENAME:-}" != "$RELEASE" ]]; then
	echo "바탕 이미지의 판(${VERSION_CODENAME:-?})이 $RELEASE가 아니다" >&2
	exit 1
fi
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
# 열쇠 꾸러미의 파일 이름은 .pgp로 옮겨 가는 중이다
keyring=$(ls /usr/share/keyrings/debian-archive-keyring.pgp /usr/share/keyrings/debian-archive-keyring.gpg 2>/dev/null | head -n 1)
# 내려받은 꾸러미를 캐시(/var/cache/apt/archives에 붙인 디렉터리)에 남긴다. Docker의 Debian 이미지는 설치 뒤 지운다
rm -f /etc/apt/apt.conf.d/docker-clean
cat >/etc/apt/apt.conf.d/90buntos <<EOF
Binary::apt::APT::Keep-Downloaded-Packages "true";
APT::Sandbox::User "root";
Acquire::Retries "5";
EOF
# 서명은 apt가 InRelease로 확인하므로 http로 받는다 (바탕 이미지에는 인증서 꾸러미가 없다)
while read -r archive suites; do
	[[ -n "$archive" ]] || continue
	cat >>/etc/apt/sources.list.d/buntos-snapshot.sources <<EOF
Types: deb
URIs: http://snapshot.debian.org/archive/$archive/$SNAPSHOT/
Suites: $suites
Components: main
Signed-By: $keyring
Check-Valid-Until: no

EOF
done <<<"$REPOSITORIES"
# 바탕 이미지에 이미 든 꾸러미도 스냅숏의 판으로 맞춘다 (더 새것이면 내린다)
cat >/etc/apt/preferences.d/buntos-snapshot <<EOF
Package: *
Pin: origin snapshot.debian.org
Pin-Priority: 1001
EOF
export DEBIAN_FRONTEND=noninteractive
apt-get -q update
apt-get -q -y --allow-downgrades dist-upgrade
apt-get -q -y --no-install-recommends install $PACKAGES
chown -R "$HOST_UID:$HOST_GID" /var/cache/apt/archives
'
	name="buntos-debian-snapshot-$$"
	# 진행 기록은 표준 오류로 낸다 (표준 출력은 이미지 이름이다)
	if ! docker run --name "$name" --init \
		--volume "$archives:/var/cache/apt/archives" \
		--env RELEASE="$release" --env SNAPSHOT="$snapshot" --env REPOSITORIES="$repositories" \
		--env PACKAGES="$*" --env HOST_UID="$(id -u)" --env HOST_GID="$(id -g)" \
		"$base" bash -c "$prepare" >&2; then
		docker rm "$name" >/dev/null 2>&1 || true
		return 1
	fi
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
	echo "$image"
}
