// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI 에이전트 접근 정책: 이 컴퓨터를 제어할 수 있는 AI와 AI가 접근할 수 없는 앱 (ARCHITECTURE 6.5).
//!
//! - 정책은 `/etc/buntos/agent-access.toml`(root:root 0644)에 있다. 시스템 설정의 AI 에이전트 페이지가
//!   `bunt-access-helper`를 pkexec로 실행해 고친다. polkit 동작은 `org.buntos.agent.access.change`이고
//!   바꿀 때마다 관리자 암호를 묻는다.
//! - 에이전트 데몬(buntd-agent)은 소켓에 붙은 프로세스를 [`identify`]로 알아보고, [`Policy::allows`]로
//!   받을지, [`Policy::blocks`]로 앱에 접근하게 할지 정한다. 받지 않은 클라이언트는 [`record_request`]로
//!   남기고, 설정 페이지가 [`requests`]로 읽어 더할 AI로 보여 준다.
//! - 허용 목록은 실행 파일 경로로 맞춘다. 사용자 홈 아래의 스스로 업데이트하는 AI(클로드 코드 등)는
//!   판 번호·빌드 해시 자리를 `*`로 바꾼 패턴으로도 맞춰 업데이트해도 허용이 풀리지 않는다
//!   ([`version_pattern`]).
//! - 데몬 안에서 쓰므로 비동기와 네트워크 없이 파일만 읽고 쓴다.
//!
//! 데몬이 지키는 정책이다. 같은 사용자로 도는 프로그램이 접근성 버스(AT-SPI)에 직접 붙으면 막지
//! 못한다. 단단한 격리는 에이전트 seat와 샌드박스에서 한다 (ARCHITECTURE 6.6절 5단계).

use std::path::PathBuf;

mod desktop;
mod files;
mod identify;
mod pattern;
mod policy;
mod requests;

pub use desktop::{DesktopApp, desktop_app};
pub use identify::{ProgramError, can_identify, identify, program_client};
pub use pattern::version_pattern;
pub use policy::{
    AllowedClient, POLICY_PATH, Policy, PolicyError, normalize_app_id, valid_app_id, valid_name,
    valid_path,
};
pub use requests::{REQUESTS_FILE, Request, record_request, requests, requests_path};

/// 정책을 고치는 특권 도우미. polkit 동작의 exec.path 주석과 같은 경로다
pub const HELPER_PATH: &str = "/usr/libexec/buntos/bunt-access-helper";

/// 에이전트 데몬에 붙은 프로그램
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// 보이는 이름: Exec가 맞는 .desktop 앱 이름, 없으면 스크립트나 실행 파일의 이름
    pub name: String,
    /// 실행 파일의 절대 경로 (/proc/<pid>/exe). 알아보지 못했으면 비어 있다
    pub exe: PathBuf,
    /// 인터프리터(node, python 등)가 실행한 스크립트의 절대 경로
    pub script: Option<PathBuf>,
    /// root가 시킨 요청. root는 이미 무엇이든 하므로 늘 허용한다
    pub admin: bool,
}

impl Client {
    /// 실행 파일을 알아보지 못한 클라이언트 (부모가 끝나 init에 붙은 프로세스 등). 관리자가 아니면
    /// 허용 목록과 맞을 수 없고 요청 기록에도 남지 않는다
    pub fn is_unknown(&self) -> bool {
        self.exe.as_os_str().is_empty()
    }
}
