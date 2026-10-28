// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! greetd IPC (greetd-ipc(7)). greetd가 GREETD_SOCK에 알려 주는 유닉스 소켓으로, 메시지마다
//! 네이티브 바이트 순서의 32비트 길이 다음에 JSON을 보낸다. 로그인 화면은 세션을 만들고(사용자
//! 이름), PAM이 묻는 암호에 답하고, 성공하면 사용자 세션 명령을 예약한 뒤 끝난다

use std::{
    fmt,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
};

use serde::{Deserialize, Serialize};

/// greetd가 로그인 화면에 알려 주는 소켓 경로
pub const SOCKET_VARIABLE: &str = "GREETD_SOCK";
/// 한 메시지의 최대 크기. greetd의 답은 짧다
const MESSAGE_MAX: usize = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request<'a> {
    CreateSession {
        username: &'a str,
    },
    PostAuthMessageResponse {
        response: Option<&'a str>,
    },
    StartSession {
        cmd: &'a [String],
        env: &'a [String],
    },
    CancelSession,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error {
        error_type: ErrorType,
        description: String,
    },
    AuthMessage {
        auth_message_type: AuthMessageType,
        auth_message: String,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    /// 인증 실패 (암호가 틀림)
    AuthError,
    Error,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    /// 보이는 입력 (예: 사용자 이름)
    Visible,
    /// 가린 입력 (암호)
    Secret,
    Info,
    Error,
}

pub struct Client {
    stream: UnixStream,
}

impl Client {
    pub fn connect(path: &Path) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(path)?,
        })
    }

    /// greetd가 띄운 로그인 화면이면 GREETD_SOCK이 있다
    pub fn from_env() -> io::Result<Self> {
        let path = std::env::var_os(SOCKET_VARIABLE)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "GREETD_SOCK is not set"))?;
        Self::connect(Path::new(&path))
    }

    pub fn create_session(&mut self, username: &str) -> io::Result<Response> {
        self.request(&Request::CreateSession { username })
    }

    pub fn respond(&mut self, response: Option<&str>) -> io::Result<Response> {
        self.request(&Request::PostAuthMessageResponse { response })
    }

    pub fn start_session(&mut self, cmd: &[String], env: &[String]) -> io::Result<Response> {
        self.request(&Request::StartSession { cmd, env })
    }

    pub fn cancel_session(&mut self) -> io::Result<Response> {
        self.request(&Request::CancelSession)
    }

    fn request(&mut self, request: &Request) -> io::Result<Response> {
        let payload = serde_json::to_vec(request)?;
        let length = u32::try_from(payload.len()).map_err(io::Error::other)?;
        self.stream.write_all(&length.to_ne_bytes())?;
        self.stream.write_all(&payload)?;
        let mut length = [0; 4];
        self.stream.read_exact(&mut length)?;
        let length = u32::from_ne_bytes(length) as usize;
        if length > MESSAGE_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "greetd message is too large",
            ));
        }
        let mut payload = vec![0; length];
        self.stream.read_exact(&mut payload)?;
        Ok(serde_json::from_slice(&payload)?)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoginError {
    /// 암호가 틀렸다
    Auth,
    /// 그 밖의 실패. greetd나 PAM이 준 설명
    Failed(String),
}

impl fmt::Display for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth => f.write_str("authentication failed"),
            Self::Failed(description) => f.write_str(description),
        }
    }
}

impl From<io::Error> for LoginError {
    fn from(err: io::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

/// 사용자를 암호로 인증하고 세션을 예약한다. 성공하면 로그인 화면이 끝난 뒤 greetd가 같은 VT에서
/// `cmd`를 사용자 세션으로 띄운다. 실패하면 만들던 세션을 취소해 다음 시도를 받을 수 있게 한다
pub fn login(
    client: &mut Client,
    user: &str,
    password: &str,
    cmd: &[String],
    env: &[String],
) -> Result<(), LoginError> {
    let result = authenticate(client, user, password).and_then(|()| {
        match client.start_session(cmd, env)? {
            Response::Success => Ok(()),
            Response::Error { description, .. } => Err(LoginError::Failed(description)),
            Response::AuthMessage { auth_message, .. } => Err(LoginError::Failed(auth_message)),
        }
    });
    if result.is_err() {
        let _ = client.cancel_session();
    }
    result
}

fn authenticate(client: &mut Client, user: &str, password: &str) -> Result<(), LoginError> {
    let mut response = client.create_session(user)?;
    let mut answered = false;
    // PAM이 암호 말고 다른 것을 알리면(예: 암호 만료) 그 글을 실패 설명으로 쓴다
    let mut notes: Vec<String> = Vec::new();
    loop {
        response = match response {
            Response::Success => return Ok(()),
            Response::Error {
                error_type: ErrorType::AuthError,
                ..
            } => return Err(LoginError::Auth),
            Response::Error { description, .. } => return Err(LoginError::Failed(description)),
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                ..
            } if !answered => {
                answered = true;
                client.respond(Some(password))?
            }
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Info | AuthMessageType::Error,
                auth_message,
            } => {
                notes.push(auth_message.trim().to_owned());
                client.respond(None)?
            }
            // 두 번째 암호 질문(새 암호 정하기 등)이나 보이는 입력은 로그인 화면이 아직 다루지 않는다
            Response::AuthMessage { auth_message, .. } => {
                notes.push(auth_message.trim().to_owned());
                notes.retain(|note| !note.is_empty());
                return Err(LoginError::Failed(notes.join(" ")));
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 요청을 받아 차례로 정해 둔 답을 보내는 가짜 greetd. 받은 요청(JSON)을 돌려준다
    fn fake_greetd(replies: Vec<&'static str>) -> (Client, std::thread::JoinHandle<Vec<String>>) {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let mut length = [0; 4];
                if theirs.read_exact(&mut length).is_err() {
                    break;
                }
                let mut payload = vec![0; u32::from_ne_bytes(length) as usize];
                theirs.read_exact(&mut payload).unwrap();
                requests.push(String::from_utf8(payload).unwrap());
                theirs
                    .write_all(&(reply.len() as u32).to_ne_bytes())
                    .unwrap();
                theirs.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (Client { stream: ours }, server)
    }

    const SECRET: &str =
        r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#;
    const SUCCESS: &str = r#"{"type":"success"}"#;

    #[test]
    fn login_answers_the_password_and_starts_the_session() {
        let (mut client, server) = fake_greetd(vec![SECRET, SUCCESS, SUCCESS]);
        let cmd = ["/usr/libexec/buntos/session".to_owned()];
        let env = ["XDG_SESSION_TYPE=wayland".to_owned()];
        assert_eq!(login(&mut client, "buntos", "secret", &cmd, &env), Ok(()));
        let requests = server.join().unwrap();
        assert_eq!(
            requests,
            [
                r#"{"type":"create_session","username":"buntos"}"#,
                r#"{"type":"post_auth_message_response","response":"secret"}"#,
                r#"{"type":"start_session","cmd":["/usr/libexec/buntos/session"],"env":["XDG_SESSION_TYPE=wayland"]}"#,
            ]
        );
    }

    #[test]
    fn wrong_password_cancels_the_session() {
        let auth_error = r#"{"type":"error","error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"}"#;
        let (mut client, server) = fake_greetd(vec![SECRET, auth_error, SUCCESS]);
        assert_eq!(
            login(&mut client, "buntos", "wrong", &[], &[]),
            Err(LoginError::Auth)
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.last().unwrap(), r#"{"type":"cancel_session"}"#);
    }

    #[test]
    fn info_messages_are_acknowledged() {
        let info = r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Welcome"}"#;
        let (mut client, server) = fake_greetd(vec![info, SECRET, SUCCESS, SUCCESS]);
        assert_eq!(login(&mut client, "buntos", "pw", &[], &[]), Ok(()));
        let requests = server.join().unwrap();
        assert_eq!(
            requests[1],
            r#"{"type":"post_auth_message_response","response":null}"#
        );
    }

    #[test]
    fn a_second_secret_question_fails_with_its_text() {
        let expired = r#"{"type":"auth_message","auth_message_type":"info","auth_message":"You are required to change your password immediately"}"#;
        let current = r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Current password: "}"#;
        let (mut client, server) = fake_greetd(vec![SECRET, expired, current, SUCCESS]);
        let result = login(&mut client, "buntos", "pw", &[], &[]);
        assert_eq!(
            result,
            Err(LoginError::Failed(
                "You are required to change your password immediately Current password:".into()
            ))
        );
        server.join().unwrap();
    }
}
