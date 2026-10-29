// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 잠금 화면의 암호 확인 (PAM, 서비스 bunt-lock: image/mkosi.extra/etc/pam.d/bunt-lock).
//!
//! 사용자로 도는 프로세스이므로 pam_unix는 setgid 도우미(unix_chkpwd)로 자기 암호만 확인한다. 확인은 막히는
//! 호출이고, 틀리면 pam_unix가 2초쯤 기다린 뒤 돌아오므로 따로 도는 스레드에서 부른다. 대화 함수는 암호를 묻는
//! 물음(PAM_PROMPT_ECHO_OFF)에만 답한다. 보이는 글을 묻는 물음(일회용 코드 등)은 잠금 화면에 칸이 없어 거절한다.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
};

const SERVICE: &str = "bunt-lock";

// Linux-PAM의 값 (security/_pam_types.h)
const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_AUTH_ERR: c_int = 7;
const PAM_CRED_INSUFFICIENT: c_int = 8;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_MAXTRIES: c_int = 11;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;
const PAM_REFRESH_CRED: c_int = 0x0010;
/// 한 번에 받는 물음 수의 상한 (Linux-PAM의 PAM_MAX_NUM_MSG)
const MAX_MESSAGES: c_int = 32;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type Conversation = unsafe extern "C" fn(
    c_int,
    *mut *const PamMessage,
    *mut *mut PamResponse,
    *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<Conversation>,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conversation: *const PamConv,
        handle: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_setcred(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(handle: *mut PamHandle, error: c_int) -> *const c_char;
}

/// 친 암호. 다 쓰면 지운다(메모리에 남겨 두지 않는다)
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            // 안전: 살아 있는 Vec의 원소를 쓴다. 컴파일러가 지우기를 빼지 못하게 volatile로 쓴다
            unsafe { ptr::write_volatile(byte, 0) };
        }
    }
}

/// 확인 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    /// 암호가 틀렸다
    Rejected,
    /// 확인하지 못했다 (PAM 설정, 대화 오류 등). 까닭
    Failed(String),
}

/// `user`의 암호가 `password`인지 PAM으로 확인한다. 막히는 호출이다
pub fn authenticate(user: &str, password: &Secret) -> Outcome {
    if password.is_empty() || password.0.contains(&0) {
        return Outcome::Rejected;
    }
    let (Ok(service), Ok(user)) = (CString::new(SERVICE), CString::new(user)) else {
        return Outcome::Failed("the user name is not valid".into());
    };
    let conversation = PamConv {
        conv: Some(converse),
        appdata_ptr: ptr::from_ref(password).cast_mut().cast(),
    };
    let mut handle = ptr::null_mut();
    // 안전: 문자열은 NUL로 끝나고 pam_end까지 산다. conversation과 그 안의 password도 이 함수 안에서 산다
    let started = unsafe { pam_start(service.as_ptr(), user.as_ptr(), &conversation, &mut handle) };
    if started != PAM_SUCCESS || handle.is_null() {
        return Outcome::Failed(format!("pam_start failed ({started})"));
    }
    // 안전: pam_start가 준 핸들이다
    let status = unsafe { pam_authenticate(handle, PAM_DISALLOW_NULL_AUTHTOK) };
    let outcome = match status {
        PAM_SUCCESS => {
            // 화면 보호기처럼 자격(Kerberos 표 등)을 새로 고친다. 실패해도 잠금은 푼다
            // 안전: 같은 핸들
            unsafe { pam_setcred(handle, PAM_REFRESH_CRED) };
            Outcome::Accepted
        }
        status => classify(status, || error_text(handle, status)),
    };
    // 안전: 같은 핸들을 한 번만 닫는다
    unsafe { pam_end(handle, status) };
    outcome
}

/// 성공이 아닌 PAM 결과의 뜻. 틀린 암호와 비슷한 것은 Rejected, 나머지는 Failed(까닭)다
fn classify(status: c_int, describe: impl FnOnce() -> String) -> Outcome {
    match status {
        PAM_SUCCESS => Outcome::Accepted,
        PAM_AUTH_ERR | PAM_CRED_INSUFFICIENT | PAM_USER_UNKNOWN | PAM_MAXTRIES => Outcome::Rejected,
        _ => Outcome::Failed(describe()),
    }
}

fn error_text(handle: *mut PamHandle, status: c_int) -> String {
    // 안전: pam_strerror는 정적 문자열이나 핸들에 딸린 문자열을 준다. 핸들을 닫기 전에 복사한다
    let text = unsafe { pam_strerror(handle, status) };
    if text.is_null() {
        return format!("PAM error {status}");
    }
    // 안전: NUL로 끝나는 C 문자열이다
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// PAM의 대화 함수. 암호를 묻는 물음에 친 암호를 답하고, 알림 글은 받기만 한다
unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if !(1..=MAX_MESSAGES).contains(&count)
        || messages.is_null()
        || responses.is_null()
        || data.is_null()
    {
        return PAM_CONV_ERR;
    }
    let count = count as usize;
    // 안전: authenticate가 appdata_ptr에 넣은 Secret이고, pam_authenticate가 도는 동안 산다
    let password = unsafe { &*data.cast::<Secret>() };
    // 안전: 답은 PAM이 free()로 지우므로 C 할당자로 0으로 채워 만든다
    let replies = unsafe { libc::calloc(count, size_of::<PamResponse>()) }.cast::<PamResponse>();
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    for index in 0..count {
        // 안전: Linux-PAM은 물음 포인터 count개의 배열을 넘긴다
        let message = unsafe { *messages.add(index) };
        let style = if message.is_null() {
            None
        } else {
            // 안전: PAM이 준 물음이다
            Some(unsafe { (*message).msg_style })
        };
        let answer = match style {
            Some(PAM_PROMPT_ECHO_OFF) => {
                let copy = c_copy(&password.0);
                if copy.is_null() {
                    // 안전: 지금까지 만든 답을 지운다
                    unsafe { free_replies(replies, count) };
                    return PAM_BUF_ERR;
                }
                copy
            }
            Some(PAM_ERROR_MSG | PAM_TEXT_INFO) => ptr::null_mut(),
            // 보이는 글을 묻는 물음이나 모르는 물음에는 답하지 않는다
            _ => {
                // 안전: 지금까지 만든 답을 지운다
                unsafe { free_replies(replies, count) };
                return PAM_CONV_ERR;
            }
        };
        // 안전: replies는 count개를 할당했다
        unsafe { (*replies.add(index)).resp = answer };
    }
    // 안전: PAM이 준 자리에 답 배열을 넘긴다. PAM이 지운다
    unsafe { *responses = replies };
    PAM_SUCCESS
}

/// NUL로 끝나는 C 문자열 사본 (malloc). PAM이 쓴 뒤 지우고 free한다
fn c_copy(bytes: &[u8]) -> *mut c_char {
    // 안전: 길이+1 바이트를 할당해 내용과 NUL을 쓴다
    unsafe {
        let copy = libc::malloc(bytes.len() + 1).cast::<u8>();
        if copy.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), copy, bytes.len());
        *copy.add(bytes.len()) = 0;
        copy.cast()
    }
}

/// 다 만들지 못한 답 배열을 지운다. 암호 사본은 0으로 덮은 뒤 지운다
///
/// # Safety
/// `replies`는 calloc으로 `count`개를 할당한 배열이어야 한다
unsafe fn free_replies(replies: *mut PamResponse, count: usize) {
    for index in 0..count {
        // 안전: 부르는 쪽이 보장한다
        let answer = unsafe { (*replies.add(index)).resp };
        if !answer.is_null() {
            // 안전: c_copy가 만든 NUL로 끝나는 문자열이다
            unsafe {
                let length = libc::strlen(answer);
                ptr::write_bytes(answer, 0, length);
                libc::free(answer.cast());
            }
        }
    }
    // 안전: calloc한 배열이다
    unsafe { libc::free(replies.cast()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_passwords_and_failures_are_told_apart() {
        assert_eq!(classify(PAM_AUTH_ERR, String::new), Outcome::Rejected);
        assert_eq!(classify(PAM_MAXTRIES, String::new), Outcome::Rejected);
        assert_eq!(classify(PAM_USER_UNKNOWN, String::new), Outcome::Rejected);
        assert_eq!(
            classify(PAM_CONV_ERR, || "conversation error".into()),
            Outcome::Failed("conversation error".into())
        );
    }

    #[test]
    fn empty_passwords_are_rejected_without_asking_pam() {
        assert_eq!(authenticate("nobody", &Secret::new("")), Outcome::Rejected);
        assert_eq!(
            authenticate("nobody", &Secret::new("a\0b")),
            Outcome::Rejected
        );
    }

    /// 대화 함수: 암호 물음에는 친 암호를, 알림에는 빈 답을 주고, 보이는 글을 묻는 물음은 거절한다
    #[test]
    fn the_conversation_answers_only_password_prompts() {
        let password = Secret::new("hunter22");
        let prompt = c"Password: ";
        let ask = |styles: &[c_int]| {
            let messages: Vec<PamMessage> = styles
                .iter()
                .map(|style| PamMessage {
                    msg_style: *style,
                    msg: prompt.as_ptr(),
                })
                .collect();
            let mut pointers: Vec<*const PamMessage> = messages.iter().map(ptr::from_ref).collect();
            let mut replies: *mut PamResponse = ptr::null_mut();
            // 안전: 물음 배열과 답 자리는 이 닫힘 안에서 산다
            let status = unsafe {
                converse(
                    styles.len() as c_int,
                    pointers.as_mut_ptr(),
                    &mut replies,
                    ptr::from_ref(&password).cast_mut().cast(),
                )
            };
            let answers: Vec<Option<String>> = if status == PAM_SUCCESS {
                (0..styles.len())
                    .map(|index| {
                        // 안전: converse가 styles.len()개를 만들었다
                        let answer = unsafe { (*replies.add(index)).resp };
                        (!answer.is_null()).then(|| {
                            // 안전: c_copy가 만든 문자열이다
                            unsafe { CStr::from_ptr(answer) }
                                .to_string_lossy()
                                .into_owned()
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            if status == PAM_SUCCESS {
                // 안전: converse가 calloc한 배열이다
                unsafe { free_replies(replies, styles.len()) };
            }
            (status, answers)
        };
        assert_eq!(
            ask(&[PAM_TEXT_INFO, PAM_PROMPT_ECHO_OFF]),
            (PAM_SUCCESS, vec![None, Some("hunter22".to_owned())])
        );
        // 보이는 글을 묻는 물음(PAM_PROMPT_ECHO_ON = 2)
        assert_eq!(ask(&[PAM_PROMPT_ECHO_OFF, 2]).0, PAM_CONV_ERR);
        assert_eq!(ask(&[]).0, PAM_CONV_ERR);
    }
}
