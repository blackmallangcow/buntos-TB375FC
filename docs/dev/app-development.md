# buntOS용 앱 개발 안내

buntOS에서 잘 도는 앱을 만들 때 알아 둘 것을 모았다. buntOS는 Debian 위의 Wayland 데스크톱이다. 컴포지터(`bunt-comp`), 셸(`bunt-shell`), 입력기 연결, AI 에이전트 계층은 buntOS가 만들고, 앱은 리눅스 앱 그대로 돈다. 화면과 단축키는 macOS 26을 따르므로 몇 가지는 다른 리눅스 데스크톱과 다르다.

이 문서는 다른 사람이 만드는 앱을 위한 것이다. buntOS가 직접 만드는 화면(셸, 시스템 설정 등)의 규칙은 [디자인 스펙](../design/DESIGN-SPEC.md)에, 구조는 [설계안](../design/ARCHITECTURE.md)에 있다. 내용은 2026-11-21의 main을 기준으로 한다.

## 한눈에 보기

| 할 일 | 까닭 |
|---|---|
| Wayland로 띄운다 | XWayland가 없어 X11로만 그리는 앱은 창이 뜨지 않는다(2장) |
| 창의 앱 ID를 `.desktop` 파일 이름과 맞춘다 | Dock, 메뉴 막대, 알림 설정, AI 에이전트가 이것으로 앱을 알아본다(4장) |
| 단축키는 리눅스 표준 Ctrl 키로 둔다 | 컴포지터가 ⌘ 단축키를 앱의 Ctrl 단축키로 바꿔 보낸다. Super 단축키는 앱에 가지 않는다(5장) |
| 글 칸은 text-input-v3를 쓴다 | 한글 입력과 화면 키보드가 이 길로만 된다(6장) |
| 창 장식은 앱이 그린다 | 서버 쪽 장식(xdg-decoration)이 없다(3장) |
| 분수 배율과 터치를 받는다 | 태블릿은 큰 배율과 터치 화면으로 쓴다(2장, 7장) |
| 접근성(AT-SPI)을 채운다 | AI 에이전트가 앱을 읽고 누르는 길이다(11장) |
| arm64와 x86-64를 다 낸다 | 다른 아키텍처를 돌리는 번역 계층이 없다(13장) |
| Flatpak, `.deb`, AppImage(2형)로 낸다 | 패키지 매니저가 이 셋을 설치한다(13장) |

## 1. 플랫폼

- **바탕**: Debian forky(testing). 이미지는 정해 둔 Debian 스냅숏으로 만들고, 설치한 뒤에는 apt와 앱 스토어가 Debian의 새 판을 받는다.
- **아키텍처**: 데스크톱 라인은 x86-64와 arm64, 태블릿(Lenovo TB375FC 등)은 arm64다.
- **세션 환경**:
  - `XDG_SESSION_TYPE=wayland`, `XDG_CURRENT_DESKTOP=buntOS:GNOME`. 뒤의 GNOME은 Chromium·Electron 앱이 비밀 저장소로 GNOME 키링을 고르게 하려는 것이다.
  - 표시 언어의 기본은 `ko_KR.UTF-8`이고 시스템 설정에서 바꾼다. 커서 테마는 Adwaita, 크기 24다.
  - 입력기 환경 변수(`GTK_IM_MODULE`, `QT_IM_MODULE`)는 두지 않는다. 글 입력은 Wayland의 text-input-v3로 한다(6장).
- **기본 앱**: Firefox ESR, GNOME 앱(파일, 텍스트 편집기, 계산기, 이미지 보기, 문서 보기 등), 터미널 foot.

## 2. 툴킷과 Wayland

- **XWayland가 없다.** X11로만 그리는 툴킷(Tk, Java AWT/Swing 등)과 앱은 창이 뜨지 않는다.
- **GTK3·GTK4**는 그대로 된다. buntOS의 앱도 GTK4로 만든다.
- **Qt**는 Wayland 플랫폼 플러그인이 있어야 뜬다. 이미지에는 없으므로 `.deb`라면 `qt6-wayland`(Qt 5는 `qtwayland5`)를 의존성으로 건다. Flatpak의 KDE 런타임에는 들어 있다.
- **Chromium·Electron**은 Wayland(Ozone)로 떠야 한다. VS Code가 TB375FC에서 돈다.
- **SDL**은 X11이 없으면 Wayland로 뜬다.
- **그리기**: Mesa의 EGL·OpenGL(ES)과 Vulkan 드라이버(`mesa-vulkan-drivers`)가 들어 있다. 앱이 GPU로 그린 버퍼는 dmabuf로 넘긴다(12장).

### 2.1 앱이 쓸 수 있는 프로토콜

| 프로토콜 | 쓰임 |
|---|---|
| `wl_compositor`, `wl_subcompositor`, `wl_shm`, `wl_seat`(키보드, 포인터, 터치), `wl_output`, `zxdg_output_manager_v1` | 기본 |
| `xdg_wm_base`(xdg-shell), `xdg_wm_dialog_v1` | 창과 팝업, 모달 대화 상자 |
| `zwp_linux_dmabuf_v1`(5판) | GPU 버퍼를 복사 없이 넘긴다. 기본 피드백은 컴포지터가 그리는 GPU다 |
| `wp_viewporter`, `wp_fractional_scale_manager_v1` | 분수 배율. 배율은 화면의 물리 크기와 해상도로 정한다 |
| `wp_cursor_shape_manager_v1` | 커서 모양을 이름으로 고른다 |
| `zwp_text_input_manager_v3` | 글 입력(한글 입력기, 화면 키보드) |
| `zwp_keyboard_shortcuts_inhibit_manager_v1` | 단축키를 막고 키를 그대로 받는다(5.4절) |
| `wl_data_device_manager` | 복사·붙여넣기와 끌어 놓기 |
| `ext_data_control_manager_v1`, `zwlr_data_control_manager_v1` | 포커스 없이 클립보드를 읽고 쓴다. 샌드박스 앱에는 보이지 않는다(8장) |
| `wp_security_context_manager_v1` | Flatpak이 앱마다 따로 소켓을 만든다 |
| `ext_background_effect_manager_v1` | 창의 반투명한 곳 뒤를 흐린다(10장) |
| `zwlr_layer_shell_v1` | 패널이나 오버레이를 띄운다. 셸의 메뉴 막대, Dock과 자리가 겹칠 수 있다 |

### 2.2 없는 것

| 없는 것 | 앱에 미치는 것 |
|---|---|
| XWayland | X11 앱이 뜨지 않는다 |
| `zxdg_decoration_manager_v1` | 서버 쪽 창 장식이 없다. 앱이 제목 막대와 창 단추를 그린다 |
| `xdg_activation_v1` | 다른 앱에 활성화 토큰을 넘기거나 창을 앞으로 가져오라고 청할 수 없다. 새 창이 포커스를 받는 규칙은 3장에 있다 |
| `zwp_primary_selection_device_manager_v1` | 가운데 단추로 붙여넣는 기본 선택(primary selection)이 없다. 일부러 열지 않는다 |
| `zwp_pointer_constraints_v1`, `zwp_relative_pointer_manager_v1` | 포인터를 가두거나 상대 움직임을 받을 수 없다. 1인칭 게임과 3D 도구의 시점 돌리기가 어렵다 |
| `zwp_pointer_gestures_v1` | 트랙패드의 두 손가락 확대·밀기 제스처가 앱에 가지 않는다(스크롤과 탭은 간다) |
| `zwp_tablet_manager_v2` | 펜 입력이 앱에 가지 않는다(7장) |
| `wp_presentation`, `xdg_toplevel_icon_manager_v1`, `xdg_system_bell_v1`, `zwp_idle_inhibit_manager_v1` | 화면 표시 시각, 창별 아이콘, 시스템 경고음 요청, 화면 꺼짐 막기를 프로토콜로 할 수 없다 |
| 화면 캡처(screencopy, image-capture) | 앱이 화면을 찍을 수 없다. 화면 캡처·화면 공유 포털도 없다(9장) |

## 3. 창

- 창 기능(`wm_capabilities`): 최대화, 전체 화면, 최소화. 창 메뉴는 없다. 최소화 단추를 보이면 컴포지터가 창을 Dock의 최소화 칸으로 내린다.
- 창 단추는 macOS처럼 왼쪽에 닫기, 최소화, 최대화 차례로 둔다(`org.gnome.desktop.wm.preferences button-layout='close,minimize,maximize:'`). 제목 막대를 직접 그리는 앱은 이 값을 따른다. GTK 앱에는 세션이 사용자 CSS로 macOS 신호등 단추를 입힌다(10장).
- ⌘Q는 컴포지터가 그 앱의 창마다 닫기(`xdg_toplevel.close`)를 보낸다. 앱이 창을 다 닫으면 끝나게 하거나, 저장할지 묻는다.
- 로그아웃은 모든 앱 창에 닫기를 보내고 창이 다 닫히면 끝난다. 10초 안에 닫히지 않는 창(저장할지 묻는 앱)이 있으면 그만두고 알림으로 알린다.
- 새로 뜬 창은 키보드 포커스를 받는다. 다만 사람이 다른 앱에 글을 치는 동안 뜬 창은 포커스를 빼앗지 않고 그 창 뒤에 뜬다. 사람이 치던 앱의 새 창(⌘N, 대화 상자)은 포커스를 받는다.
- `xdg_wm_base`의 핑에 3초 안에 답하지 않는 앱은 강제 종료 창(⌥⌘⎋)에 응답 없음으로 보인다.
- 대화 상자는 부모 창(`set_parent`)을 주고, 모달이면 `xdg_wm_dialog_v1`로 알린다.

## 4. 앱 ID, `.desktop`, 아이콘

- 창의 앱 ID(`xdg_toplevel.set_app_id`)를 `.desktop` 파일 이름(확장자를 뺀 것)과 같게 한다. 다르면 `.desktop`의 `StartupWMClass`에 앱 ID를 적는다. Dock, 메뉴 막대의 앱 이름, 앱 전환 창, 알림 설정, AI 에이전트(그리고 사용자가 AI에게서 막은 앱)가 모두 이것으로 앱을 찾는다. 맞지 않으면 Dock에 이름 없는 기본 아이콘이 뜬다.
  - GTK는 `GApplication`의 application-id, Qt는 `QGuiApplication::setDesktopFileName()`, Flatpak은 앱 ID가 쓰인다.
- 아이콘은 hicolor 테마에 이름으로 넣는다(`/usr/share/icons/hicolor/scalable/apps/<앱 ID>.svg` 같은 자리). SVG 하나가 가장 낫다.
- `.desktop`에는 `Name`, `Name[ko]`, `Icon`, `Categories`, `MimeType`을 채운다. 모든 앱 패널과 기본 앱 설정이 쓴다.
- `DBusActivatable=true` 앱은 AI 에이전트도 `org.freedesktop.Application.Activate`로 연다.

## 5. 키보드와 단축키

⌘는 Super 키다. 맥 키보드의 Command, PC 키보드의 Windows 키이고, ⌘ 키가 없는 태블릿 키보드 커버(Ctrl, 지구본, Alt)에서는 지구본 키다. 컴포지터가 ⌘ 단축키를 받아 포커스를 가진 앱이 아는 키로 바꿔 보낸다. 앱은 ⌘를 누른 적이 없는 것처럼 그 키를 받는다(누르고 있던 ⌘는 잠시 뗀 것으로 보낸다). 사람은 Ctrl 단축키도 그대로 누를 수 있다. 그래서 **앱은 리눅스 표준 Ctrl 단축키만 두면 ⌘와 Ctrl 모두로 쓰인다.** Super를 쓰는 앱 단축키는 앱에 가지 않는다.

### 5.1 앱이 받는 키

| 사람이 누른 키 | 앱이 받는 키 |
|---|---|
| ⌘X, ⌘C, ⌘V | Ctrl+X, Ctrl+C, Ctrl+V |
| ⌘Z, ⇧⌘Z | Ctrl+Z, Ctrl+Shift+Z |
| ⌘A | Ctrl+A |
| ⌘F, ⌘G, ⇧⌘G | Ctrl+F, Ctrl+G, Ctrl+Shift+G |
| ⌘N, ⌘T, ⌘O | Ctrl+N, Ctrl+T, Ctrl+O |
| ⌘S, ⇧⌘S, ⌘P | Ctrl+S, Ctrl+Shift+S, Ctrl+P |
| ⌘W | Ctrl+W |
| ⌘,(쉼표) | Ctrl+,(설정) |
| ⌘R | Ctrl+R |
| ⌘=, ⌘-, ⌘0 | Ctrl+=, Ctrl+-, Ctrl+0 |
| ⌘[, ⌘] | Alt+←, Alt+→(뒤로, 앞으로) |
| 표에 없는 ⌘ 글자 키 | 같은 글자의 Ctrl 단축키(⌘B → Ctrl+B). 사용자가 시스템 설정에서 끌 수 있다 |

텍스트 편집 키는 앱이 글 입력(text-input-v3)을 켜 두었을 때만 바꾼다. 켜지 않았으면 앱이 원래 키를 받는다(⌥←는 Alt+←).

| 사람이 누른 키 | 앱이 받는 키 |
|---|---|
| ⌘←, ⌘→ | Home, End |
| ⌘↑, ⌘↓ | Ctrl+Home, Ctrl+End |
| ⌥←, ⌥→ / ⌥↑, ⌥↓ | Ctrl+←, Ctrl+→ / Ctrl+↑, Ctrl+↓ |
| ⌥⌫, ⌥⌦ | Ctrl+Backspace, Ctrl+Delete |
| ⌘⌫ | Shift+Home 뒤 Backspace |

⇧를 더하면 보낸 키에도 Shift가 붙어 선택이 넓어진다.

### 5.2 앱에 가지 않는 키

컴포지터가 직접 쓰는 키다: ⌘Q(앱의 창을 모두 닫는다), ⌘H, ⌥⌘H, ⌘M, ⌘⇥, ⌘\`, ⌘스페이스, ⌃⌘F, ⇧⌘3·4·5, ⌃⌘Q, ⇧⌘Q, ⌥⌘⎋, ⌃⌥⌘.(AI 제어 멈춤), ⌃스페이스와 한/영 키(입력 소스), 음량 키, Ctrl+Alt+F1~F12, Ctrl+Alt+Backspace. Caps Lock 키는 짧게 누르면 입력 소스를 바꾸고 길게(0.4초) 누르면 Caps Lock을 켠다. 한/영 키, 음량 키, 가상 터미널 전환, 세션 끝내기를 빼면 사용자가 시스템 설정의 키보드 단축키 시트에서 바꾸거나 끌 수 있으므로, 앱은 이 키들이 늘 비어 있다고 기대하지 않는다.

### 5.3 키 되풀이

- 컴포지터가 바꿔 보낸 키는 한 번 눌렀다 뗀 것으로 간다. ⌘Z, ⇧⌘Z, ⌘G, ⇧⌘G, ⌘=, ⌘-와 텍스트 편집 키는 사람이 누르고 있으면 컴포지터가 키 반복 설정(기본 지연 0.4초, 초당 25번)대로 되풀이해 보내고, ⌘C·⌘V 같은 나머지는 한 번만 보낸다.
- 앱이 글 입력을 켠 동안 키는 입력기(fcitx5)를 거친다. 이때 되풀이는 처음 누른 시각을 단 "뗌+누름" 쌍으로 온다. 앱은 이것을 새로 누른 것과 가를 수 없으니, 되풀이와 새 누름을 다르게 다루는 기능(길게 누르기 등)은 이 점을 생각한다.

### 5.4 키를 그대로 받아야 하는 앱

원격 데스크톱, 가상 머신 화면, 게임처럼 ⌘ 단축키까지 받아야 하는 앱은 `zwp_keyboard_shortcuts_inhibit_manager_v1`로 단축키를 막는다. 컴포지터는 바로 허락하고, 그 창이 포커스를 가진 동안 가상 터미널 전환만 빼고 모든 키를 그대로 보낸다.

### 5.5 터미널 에뮬레이터

터미널은 Ctrl+글자를 제어 문자로 쓰므로 컴포지터가 앱 ID로 터미널을 알아보고 다른 키를 보낸다. 지금 아는 앱 ID는 `foot`, `footclient`, `org.codeberg.dnkl.foot`, `kitty`, `Alacritty`, `org.gnome.Console`, `org.gnome.Ptyxis`, `org.gnome.Terminal`, `org.kde.konsole`, `com.mitchellh.ghostty`, `org.wezfurlong.wezterm`, `xterm`이다(`comp/src/shortcuts.rs`의 `TERMINALS`).

| 사람이 누른 키 | 터미널이 받는 키 |
|---|---|
| ⌘C, ⌘V | Ctrl+Shift+C, Ctrl+Shift+V |
| ⌘F | Ctrl+Shift+R(foot의 찾기) |
| ⌘N | Ctrl+Shift+N |
| ⌘W | 컴포지터가 창을 닫는다 |
| ⌘⌫ | Ctrl+U |
| ⌘=, ⌘-, ⌘0 | Ctrl+=, Ctrl+-, Ctrl+0 |
| ⌘←, ⌘→ | Home, End |

새 터미널을 만들면 이 기본 키를 쓰고 목록에 넣어 달라고 알린다.

## 6. 글 입력과 화면 키보드

- 한글 입력기는 fcitx5와 fcitx5-hangul(두벌식)이다. 입력기 프로토콜(input-method-v2)은 컴포지터가 띄운 입력기만 쓰고, 앱은 **text-input-v3**로 입력기와 이어진다. text-input-v3를 쓰지 않는 앱에는 한글을 칠 수 없다.
- buntOS에서 시험한 앱: GTK4 앱(텍스트 편집기), Firefox, foot. 한글 조합, 조합 중 Enter·Tab, 조합 중 다른 창 누르기까지 e2e로 본다. Qt와 Electron 앱의 한글 입력은 그 툴킷의 text-input-v3 지원에 달려 있고, buntOS에서는 아직 시험하지 않았다.
- 앱이 할 일:
  - 글 칸이 포커스를 받으면 글 입력을 켜고(`enable`), 잃으면 끈다(`disable`).
  - 칸의 종류(`set_content_type`)를 알린다. 암호 칸(`password`)이면 입력기는 영문으로 치고, 화면 키보드는 ABC 배열과 붙여넣기 단추만 보인다.
  - 글 커서 자리(`set_cursor_rectangle`)와 앞뒤 글(`set_surrounding_text`)을 알린다. 후보 창과 화면 키보드가 이것을 쓴다.
  - 조합 중인 글(preedit)을 칸 안에 그리고, 확정 글(commit)을 `done`의 serial대로 받는다.
- **화면 키보드**(iPadOS형): 터치 모드(7장)에서 사람이 글 칸을 누르면 셸이 화면 아래에 띄운다.
  - 칸을 눌렀다고 보는 것은 누른 창이 누르는 동안이나 뗀 뒤 3초 안에 글 입력을 켰을 때다. GTK는 누르는 동안, Firefox는 뗀 뒤에 켠다.
  - 창이 열리며 칸이 저절로 포커스를 받은 것만으로는 뜨지 않는다. iPadOS는 포커스만으로 띄우지만 buntOS는 사람이 칸을 누를 때만 띄운다.
  - 키보드가 뜨면 최대화·전체 화면 창은 키보드 위까지로 줄어든다(컴포지터가 창 크기를 다시 알린다). 보통(떠 있는) 창은 옮기지 않으므로 키보드에 가려질 수 있다. 터치용 화면은 글 칸을 창 맨 아래에 두지 않는다.

## 7. 터치, 펜, 입력 모드

- **입력 모드**: 터치 화면이 있고 물리 키보드와 포인터 장치가 하나도 없으면 터치 모드, 아니면 포인터 모드다. 키보드 커버를 붙이고 떼면 바뀐다. 앱이 모드를 읽는 표준 API는 없으니, 터치 입력(`wl_touch`)을 받으면 터치에 맞게 다룬다.
- 터치 모드에서는 커서가 보이지 않고 호버가 없다. 누를 곳은 44 논리 픽셀 이상(메뉴의 줄은 36), 손가락으로 칠 단추 사이는 띄운다(Apple HIG와 같다).
- 터치는 손가락 아래의 표면이 받고, 누른 창이 키보드 포커스를 받는다.
- 메뉴 막대나 Dock을 숨긴 동안 화면 위 가장자리 24픽셀에서 아래로, 아래 가장자리 20픽셀에서 위로 미는 것은 셸이 가져간다.
- **펜**: 아직 앱에 가지 않는다. 컴포지터가 펜(태블릿) 프로토콜을 열지 않는다. TB375FC의 펜은 기기에서 위치, 필압, 기울기를 내지만 데스크톱에서는 쓰지 못한다.
- 트랙패드: 탭으로 클릭과 두 손가락 스크롤은 된다. 확대·밀기 제스처는 앱에 가지 않는다(2.2절).

## 8. 클립보드

- 앱끼리의 복사와 붙여넣기는 `wl_data_device`다. Wayland 규칙대로 키보드 포커스가 있을 때 사람의 입력 serial로 선택을 정한다.
- 글은 `text/plain;charset=utf-8`로 내고, 그림은 `image/png`로 낸다. 컴포지터의 스크린샷(⌃⇧⌘3·4)도 `image/png`로 클립보드에 넣는다.
- 기본 선택(primary selection)이 없다. 글을 골라 가운데 단추로 붙이는 동작은 되지 않는다.
- 포커스 없이 읽고 쓰는 `ext/zwlr_data_control`은 클립보드 관리자와 `wl-copy`·`wl-paste`(이미지에 있다)가 쓴다. 샌드박스 앱, 입력기, 잠금 화면에는 보이지 않고, 화면이 잠긴 동안 새로 붙은 프로그램에도 보이지 않는다.
- 사용자가 켜면 셸의 클립보드 기록이 복사를 남긴다. 암호처럼 남기면 안 되는 복사는 MIME 형식 `x-kde-passwordManagerHint`(값 `secret`)를 함께 내면 기록이 내용을 읽지 않는다(KDE Klipper가 정했고 KeePassXC 등이 붙이는 표시다).

## 9. 알림, 메뉴 막대 항목, 포털

- **알림**: 셸이 `org.freedesktop.Notifications`(1.2)를 맡는다. 능력은 `actions`, `body`, `persistence`다. 본문 마크업과 그림은 보이지 않으니 본문은 글로 낸다. `desktop-entry` 힌트(값은 앱 ID)를 주면 시스템 설정 › 알림의 앱별 설정(허용, 배너, 소리, 알림 센터)이 그 앱에 걸린다. GTK의 `GNotification`은 앱 ID로 이 힌트를 주고, libnotify를 쓰는 앱은 힌트를 직접 단다.
- **메뉴 막대 항목**: StatusNotifierItem(`org.kde.StatusNotifierWatcher`)을 받는다. 항목은 메뉴 막대의 시스템 항목 왼쪽에 아이콘으로 뜨고, 누르면 앱이 `com.canonical.dbusmenu`로 낸 메뉴가 열린다.
- **포털**: `xdg-desktop-portal`과 gtk 백엔드다. 비밀 저장소는 gnome-keyring이다.
  - 있는 것: 파일 선택(FileChooser), 열기(OpenURI), 인쇄, 알림, 설정(Settings, 다크 모드), 비밀(Secret), 휴지통, 메일, 계정, 막기(Inhibit), 네트워크·전원·메모리 감시, 프록시, GameMode, Realtime, 카메라, 위치, DynamicLauncher.
  - 없는 것: 화면 캡처(Screenshot), 화면 공유(ScreenCast), 원격 데스크톱, 전역 단축키(GlobalShortcuts), 배경 실행(Background), 배경화면. 화상 회의 앱의 화면 공유는 아직 되지 않는다.

## 10. 모양

- **다크 모드**: `org.gnome.desktop.interface color-scheme`(`prefer-dark` 또는 `default`)이고, 포털의 설정(`org.freedesktop.appearance color-scheme`)으로도 읽는다. 사용자가 제어 센터에서 바로 바꾸므로 바뀌면 곧바로 따른다.
- **글꼴**: UI는 Pretendard 11, 고정폭은 Noto Sans Mono CJK KR 11이고 Noto CJK가 있다. 글꼴 이름을 박지 말고 fontconfig의 `sans-serif`, `monospace`나 시스템 설정의 글꼴을 쓴다.
- **창 단추**: GTK3·GTK4 앱에는 세션이 사용자 CSS(`~/.config/gtk-3.0/gtk.css`, `~/.config/gtk-4.0/gtk.css`)로 macOS 신호등 단추를 입힌다(`/usr/share/buntos/gtk-*/buntos.css`를 읽는다). 앱이 자기 CSS로 창 단추를 다시 그리면 이 모양이 사라진다.
- **흐림**: 반투명한 사이드바나 도구 막대 뒤를 흐리려면 `ext_background_effect_manager_v1`로 영역을 알린다. 컴포지터 설정에서 흐림이 꺼져 있으면 능력에 흐림이 없다고 알린다.
- **커서**: `wp_cursor_shape_manager_v1`로 모양 이름을 고르는 것이 가장 낫다.
- **소리**: XDG 소리 테마 `buntos`다. 이벤트 소리(libcanberra, GSound)는 `bell`, `message`, `message-new-instant`, `complete`, `dialog-information`, `dialog-warning`, `window-attention`, `system-ready` 같은 표준 이름으로 낸다.
- buntOS의 화면은 macOS 26과 Apple HIG를 따른다([디자인 스펙](../design/DESIGN-SPEC.md)). 다른 앱이 따를 의무는 없지만, 색·모서리·글자 크기를 맞추려면 `theme/tokens.css`의 CSS 변수를 참고한다.

## 11. AI 에이전트와 접근성

buntOS에는 AI가 사람과 한 컴퓨터를 함께 쓰는 계층이 있다([설계안](../design/ARCHITECTURE.md) 6장). 에이전트는 앱을 세 길로 다룬다: CLI·D-Bus 같은 동작 경로, **접근성 트리(AT-SPI)**로 읽고 누르는 의미 경로, 창 스크린샷과 좌표로 누르는 시각 경로다. 의미 경로가 빠르고 정확하므로 접근성을 채운 앱일수록 AI가 잘 다룬다.

- 단추, 칸, 목록에 접근 가능한 이름과 역할을 준다. 그림만 있는 단추에는 이름을 꼭 붙인다.
- 칸의 값, 체크·켜짐·꺼짐 상태, 동작(누르기)을 내놓는다. GTK·Qt의 표준 위젯을 쓰면 대부분 저절로 된다.
- 캔버스에 직접 그리는 UI(게임, 그림판, 웹 캔버스)는 트리에 나오지 않아 시각 경로로만 다뤄진다. 중요한 조작은 접근 가능한 위젯으로도 낸다.
- 접근성을 지원하지 않는 앱(foot 등)은 에이전트가 "지원하지 않는다"고 알리고 키와 휠만 보낸다.
- 암호 칸은 암호 역할로 내놓는다. 비밀 값을 접근 가능한 이름이나 설명에 넣지 않는다.
- 사용자는 앱마다 AI의 접근을 막을 수 있다. 막은 앱은 앱 ID로 적으므로 4장의 앱 ID가 맞아야 한다.
- 에이전트가 앱을 어떻게 보는지는 `bunt agent apps`와 `bunt agent snapshot --app <앱 ID> -i`로 본다.

## 12. 성능과 전력

- 컴포지터는 바뀐 것이 있을 때만 그린다. 앱이 쉴 새 없이 커밋하면(멈추지 않는 애니메이션, 타이머로 다시 그리기) 화면 전체가 계속 그려져 배터리를 쓴다. 바뀐 것이 있을 때만 커밋하고 프레임 콜백을 따른다.
- GPU로 그린 버퍼는 linux-dmabuf로 복사 없이 넘긴다. 컴포지터는 앱의 GPU 작업이 끝난 버퍼를 받으므로 느린 앱 하나가 화면 전체를 막지 않는다. GPU가 없는 기기(소프트웨어 렌더러)에서는 GTK 앱에 `GSK_RENDERER=cairo`를 주고 dmabuf를 열지 않는다.
- 주사율은 화면마다 다르다(60~144 Hz, TB375FC는 기본 120 Hz). 프레임 간격을 박지 않는다.
- **TB375FC**(arm64 태블릿):
  - GPU는 Mali-G615(panthor, Mesa 26.3)이고 GLES로 확인했다. RAM은 8GB 또는 12GB다.
  - GPU 메모리는 스왑이 없으면 돌려받지 못한다. 숨은 창의 큰 텍스처와 버퍼는 놓는다.
  - 영상 하드웨어 디코딩은 VP9만 된다(VA-API 드라이버 `buntos-tb375fc-vaapi`). H.264·HEVC·AV1은 CPU로 푼다. 동영상 앱은 VA-API(libva)를 쓰고 VP9를 고르면 덜 든다.

## 13. 배포와 설치

패키지 매니저(`bunt-packages`)가 받은 설치 파일을 열어 설치하고, 앱 스토어(GNOME Software, 모든 앱에는 "소프트웨어")가 Flathub와 Debian의 앱을 찾아 설치한다.

| 형식 | 설치 | 인증 |
|---|---|---|
| Flatpak(`.flatpak`, `.flatpakref`, Flathub) | `flatpak install`. Flathub는 이미지에 들어 있다 | 관리자는 암호 없음, 표준 사용자는 `--user` |
| `.deb`(Debian forky용) | PackageKit, apt가 의존성을 푼다 | 매번 관리자 암호 |
| AppImage(2형, squashfs) | `~/Applications/<이름>`에 풀고 `.desktop`을 만든다 | 없음 |
| `.rpm`, `.snap`, `.tar.gz` 등 | 하지 않는다. 까닭과 다른 판을 찾으라는 안내를 보인다 | |

- **아키텍처**: 패키지 매니저는 `.deb`의 Architecture, AppImage 런타임의 기계 종류, Flatpak ref를 보고 다른 아키텍처는 설치하지 않는다. x86-64 프로그램을 arm64에서 돌리는 번역 계층(box64, FEX 등)이 없으므로 태블릿에서 쓰이려면 arm64판을 낸다.
- **AppImage**: Debian forky에는 옛 런타임이 찾는 `libfuse.so.2`가 없어 패키지 매니저가 풀어서 설치하고, `APPDIR=<폴더> <폴더>/AppRun`으로 띄운다. AppImage 안의 `.desktop`에서 `Name`, `Comment`, `Categories`, `MimeType`, `StartupWMClass` 같은 키를 옮겨 오므로 `StartupWMClass`(앱 ID)를 맞게 넣는다.
- **`.deb`**: buntOS 자체 파일(`/usr/bin/bunt-*`, `/usr/libexec/buntos` 등)을 덮지 않는다. apt 보호(apt-guard)가 buntOS가 쓰는 라이브러리를 잡아 두므로 그것을 지우는 의존성은 apt가 거절한다.
- Flatpak은 샌드박스 안에서 돌고 포털(9장)로 바깥에 닿는다. 샌드박스 앱에는 포커스 없는 클립보드 프로토콜이 보이지 않는다.

## 14. 다국어

- 표시 언어의 기본은 한국어(`ko_KR.UTF-8`)이고 영어(`en_US.UTF-8`)도 들어 있다. 앱의 문자열을 한국어로 번역하고 `.desktop`에 `Name[ko]`, `Comment[ko]`를 둔다.
- 한글은 조합형 입력으로 들어오고(6장), 글자 단위(자소가 아니라 음절)로 지우고 세는 것이 맞는지 본다.
- 한국어 글꼴(Pretendard, Noto CJK)이 들어 있으므로 글꼴을 따로 싣지 않아도 된다.

## 15. 시험하기

- QEMU에서 buntOS 이미지를 띄워 시험한다(arm64·x86-64, [개발 환경](environment.md) 4장). Apple Silicon Mac에서도 된다.
- `WAYLAND_DEBUG=1 <앱>`으로 앱이 어떤 프로토콜을 쓰는지 본다.
- 한글 입력(⌃스페이스나 Caps Lock으로 전환), 다크 모드, 분수 배율(시스템 설정 › 디스플레이), 터치 모드(개발용 `comp.toml`의 `[input] mode = "touch"`)에서 본다.
- AI 에이전트가 앱을 읽는지 `bunt agent snapshot --app <앱 ID> -i`로 본다.
- 태블릿(TB375FC)이 있으면 그 위에서도 본다. 터치, 화면 키보드, 배터리를 실제로 볼 수 있다.
