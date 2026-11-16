#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 8번 게스트 쪽 도우미: 데스크탑 항목 (ARCHITECTURE 9.2, 5.2). 포인터와 키는 호스트가 QMP로 넣고
# (boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 그래서 누르기·드래그·키가 실제 입력과 같은 길
# (libinput → 컴포지터 → 셸)을 거친다. 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다.
# 사용법: desktop.sh prepare | selected | focus | files | trash | show-items true|false
set -u

user=buntos
desktop=/home/$user/Desktop
# 사람이 방금 입력했으면(QMP 키·누르기) 컴포지터가 에이전트의 입력을 미루고 "try again in N ms"로 알린다(사람 우선,
# ARCHITECTURE 6.3절). AI가 하듯 그만큼 기다렸다 다시 한다(다섯 번까지). 표준 출력과 오류는 그대로 낸다
agent_retry() {
	local err rc ms try
	err=$(mktemp)
	for try in 1 2 3 4 5 6; do
		"$@" 2>"$err"
		rc=$?
		ms=$(grep -a -o 'try again in [0-9]* ms' "$err" | grep -o '[0-9]*' | head -1)
		if [[ $rc -eq 0 || -z "$ms" || $try -eq 6 ]]; then
			cat "$err" >&2
			rm -f "$err"
			return "$rc"
		fi
		sleep "$(awk -v ms="$ms" 'BEGIN { printf "%.2f", (ms + 100) / 1000 }')"
	done
}
A() { agent_retry sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
# 셸의 데스크탑 항목 창에 보이는 항목 이름 (접근성 트리 차례, |로 잇는다)
items() {
	A snapshot --app bunt-shell | awk '/^- window "bunt-desktop"/{on=1; next} /^- window/{on=0} on' |
		grep -a 'listitem "' | sed 's/^ *- listitem "\([^"]*\)".*/\1/' | paste -sd '|'
}

case "${1:-}" in
prepare)
	# 데스크탑 폴더에 파일 넷과 폴더 하나를 한꺼번에 옮겨 넣고(셸이 한 번에 읽어 이름 차례로 놓는다. 하나씩
	# 만들면 생긴 차례로 다음 빈 칸에 놓인다), 셸이 다 보일 때까지 20초 기다린다
	staging=/home/$user/.e2e-desktop
	sudo -u "$user" mkdir -p "$staging/프로젝트"
	echo "안녕하세요" | sudo -u "$user" tee "$staging/메모.txt" >/dev/null
	for n in 1 2 10; do sudo -u "$user" touch "$staging/문서 $n.txt"; done
	sudo -u "$user" mv "$staging"/* "$desktop"/
	rmdir "$staging"
	for _ in $(seq 40); do
		[[ "$(items | tr '|' '\n' | grep -c .)" == 5 ]] && break
		sleep 0.5
	done
	items
	;;
selected)
	# 고른 항목 이름 (접근성 상태 selected, 없으면 none)
	names=$(A snapshot --app bunt-shell | awk '/^- window "bunt-desktop"/{on=1; next} /^- window/{on=0} on' |
		grep -a 'listitem "' | grep -a '\[selected\]' | sed 's/^ *- listitem "\([^"]*\)".*/\1/' | paste -sd '|')
	echo "${names:-none}"
	;;
focus)
	# 키보드 포커스를 가진 앱 ID나 레이어 이름
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 GetFocus 2>&1 |
		sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'
	;;
files)
	# 데스크탑 폴더의 항목 이름 (이름 차례, |로 잇는다)
	ls "$desktop" | LC_ALL=C sort | paste -sd '|'
	;;
trash)
	# 휴지통의 항목 이름
	ls "/home/$user/.local/share/Trash/files" 2>/dev/null | LC_ALL=C sort | paste -sd '|'
	;;
show-items)
	# 시스템 설정의 "항목 보기: 데스크탑에서"를 바꾸고 데스크탑 항목 창이 보이는지 답한다 (숨은 창은 에이전트에
	# 보이지 않는다)
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		gsettings set org.buntos.shell desktop-show-items "$2"
	for _ in $(seq 20); do
		shown=$(A snapshot --app bunt-shell | grep -ac '^- window "bunt-desktop"')
		[[ ("$2" == true && "$shown" == 1) || ("$2" == false && "$shown" == 0) ]] && break
		sleep 0.5
	done
	[[ "$shown" == 1 ]] && echo shown || echo hidden
	;;
*)
	echo "사용법: $0 prepare | selected | focus | files | trash | show-items true|false" >&2
	exit 2
	;;
esac
