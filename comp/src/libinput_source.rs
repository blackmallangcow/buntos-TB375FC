// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! libinput 입력 소스. smithay의 LibinputInputBackend 소스는 libinput fd가 읽힐 때만 쌓인 이벤트를
//! 꺼낸다. 그런데 libinput이 좌석을 붙일 때(udev_assign_seat)와 세션이 돌아와 다시 켤 때(resume) 쌓은
//! 장치 추가 이벤트는 fd를 읽히게 하지 않아, 다른 장치 이벤트가 올 때까지 남았다. 시험 VM에서는 로그인
//! 뒤 2분 넘게, 로그아웃 뒤 로그인 화면에서도 30초 넘게 키보드·포인터·터치가 없었다. Weston과 wlroots는
//! 그 뒤에 한 번 꺼낸다. 이 소스는 fd와 함께 깨우기(Ping)를 듣고, 깨우면 쌓인 이벤트를 꺼낸다. 내는
//! 이벤트는 smithay 소스와 같은 `InputEvent<LibinputInputBackend>`다

use std::{io, os::fd::AsFd};

use smithay::{
    backend::{
        input::InputEvent,
        libinput::{LibinputInputBackend, PointerScrollAxis},
    },
    reexports::{
        calloop::{
            self, EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory,
            ping::{Ping, PingSource, make_ping},
        },
        input::{
            Libinput,
            event::{
                DeviceEvent, Event, EventTrait, GestureEvent, KeyboardEvent, PointerEvent,
                SwitchEvent, TabletToolEvent, TouchEvent,
                gesture::{GestureHoldEvent, GesturePinchEvent, GestureSwipeEvent},
            },
        },
    },
};

/// libinput fd와 깨우기를 함께 듣는 입력 소스
pub struct LibinputSource {
    libinput: Libinput,
    token: Option<Token>,
    wake: PingSource,
}

impl LibinputSource {
    /// 소스와 그 깨우기. 좌석을 붙인 뒤와 libinput을 다시 켠 뒤에 깨워 쌓인 장치를 꺼내게 한다
    pub fn new(libinput: Libinput) -> io::Result<(Self, Ping)> {
        let (ping, wake) = make_ping()?;
        let source = Self {
            libinput,
            token: None,
            wake,
        };
        Ok((source, ping))
    }
}

impl EventSource for LibinputSource {
    type Event = InputEvent<LibinputInputBackend>;
    type Metadata = ();
    type Ret = ();
    type Error = io::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> io::Result<PostAction>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        let mut ready = Some(token) == self.token;
        // 깨우기를 든 쪽이 없어져도 fd는 계속 듣는다
        self.wake
            .process_events(readiness, token, |(), _| ready = true)
            .map_err(io::Error::other)?;
        if ready {
            self.libinput.dispatch()?;
            for event in &mut self.libinput {
                if let Some(event) = input_event(event) {
                    callback(event, &mut ());
                }
            }
        }
        Ok(PostAction::Continue)
    }

    fn register(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        let token = factory.token();
        self.token = Some(token);
        // 안전: fd는 이 소스가 가진 libinput과 함께 살아, 이벤트 루프에서 빼기 전에는 닫히지 않는다
        unsafe { poll.register(self.libinput.as_fd(), Interest::READ, Mode::Level, token)? };
        self.wake.register(poll, factory)
    }

    fn reregister(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        let token = factory.token();
        self.token = Some(token);
        poll.reregister(self.libinput.as_fd(), Interest::READ, Mode::Level, token)?;
        self.wake.reregister(poll, factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.token = None;
        poll.unregister(self.libinput.as_fd())?;
        self.wake.unregister(poll)
    }
}

/// libinput 이벤트를 smithay의 입력 이벤트로 옮긴다(smithay 소스와 같은 짝). 모르는 이벤트는 버린다
fn input_event(event: Event) -> Option<InputEvent<LibinputInputBackend>> {
    Some(match event {
        Event::Device(DeviceEvent::Added(event)) => {
            let device = event.device();
            tracing::info!("input device added: {}", device.sysname());
            InputEvent::DeviceAdded { device }
        }
        Event::Device(DeviceEvent::Removed(event)) => {
            let device = event.device();
            tracing::info!("input device removed: {}", device.sysname());
            InputEvent::DeviceRemoved { device }
        }
        Event::Keyboard(KeyboardEvent::Key(event)) => InputEvent::Keyboard { event },
        Event::Pointer(PointerEvent::Motion(event)) => InputEvent::PointerMotion { event },
        Event::Pointer(PointerEvent::MotionAbsolute(event)) => {
            InputEvent::PointerMotionAbsolute { event }
        }
        Event::Pointer(PointerEvent::Button(event)) => InputEvent::PointerButton { event },
        Event::Pointer(PointerEvent::ScrollWheel(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Wheel(event),
        },
        Event::Pointer(PointerEvent::ScrollFinger(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Finger(event),
        },
        Event::Pointer(PointerEvent::ScrollContinuous(event)) => InputEvent::PointerAxis {
            event: PointerScrollAxis::Continuous(event),
        },
        Event::Touch(TouchEvent::Down(event)) => InputEvent::TouchDown { event },
        Event::Touch(TouchEvent::Motion(event)) => InputEvent::TouchMotion { event },
        Event::Touch(TouchEvent::Up(event)) => InputEvent::TouchUp { event },
        Event::Touch(TouchEvent::Cancel(event)) => InputEvent::TouchCancel { event },
        Event::Touch(TouchEvent::Frame(event)) => InputEvent::TouchFrame { event },
        Event::Gesture(GestureEvent::Swipe(GestureSwipeEvent::Begin(event))) => {
            InputEvent::GestureSwipeBegin { event }
        }
        Event::Gesture(GestureEvent::Swipe(GestureSwipeEvent::Update(event))) => {
            InputEvent::GestureSwipeUpdate { event }
        }
        Event::Gesture(GestureEvent::Swipe(GestureSwipeEvent::End(event))) => {
            InputEvent::GestureSwipeEnd { event }
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::Begin(event))) => {
            InputEvent::GesturePinchBegin { event }
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::Update(event))) => {
            InputEvent::GesturePinchUpdate { event }
        }
        Event::Gesture(GestureEvent::Pinch(GesturePinchEvent::End(event))) => {
            InputEvent::GesturePinchEnd { event }
        }
        Event::Gesture(GestureEvent::Hold(GestureHoldEvent::Begin(event))) => {
            InputEvent::GestureHoldBegin { event }
        }
        Event::Gesture(GestureEvent::Hold(GestureHoldEvent::End(event))) => {
            InputEvent::GestureHoldEnd { event }
        }
        Event::Tablet(TabletToolEvent::Axis(event)) => InputEvent::TabletToolAxis { event },
        Event::Tablet(TabletToolEvent::Proximity(event)) => {
            InputEvent::TabletToolProximity { event }
        }
        Event::Tablet(TabletToolEvent::Tip(event)) => InputEvent::TabletToolTip { event },
        Event::Tablet(TabletToolEvent::Button(event)) => InputEvent::TabletToolButton { event },
        Event::Switch(SwitchEvent::Toggle(event)) => InputEvent::SwitchToggle { event },
        _ => return None,
    })
}
