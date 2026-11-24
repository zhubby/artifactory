//! [INPUT]: 依赖公开终端身份、尺寸及帧契约。
//! [OUTPUT]: 验证终端启动、窗口和字节预算的拒绝边界。
//! [POS]: 设备终端的跨服务契约回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::{TerminalFrame, terminal::*};
use uuid::Uuid;

fn request() -> OpenTerminal {
    OpenTerminal {
        terminal_id: Uuid::new_v4(),
        node_session: Uuid::new_v4(),
        startup_id: Uuid::new_v4(),
        expires_at_ms: 15_000,
        columns: 80,
        rows: 24,
        term: "xterm-256color".into(),
        profile: "default".into(),
    }
}

#[test]
fn terminal_start_requires_current_identity_and_bounded_window() {
    let mut open = request();
    assert!(open.validate(0).is_ok());
    assert!(open.validate(15_000).is_err(), "期限到达后不能启动");
    open.columns = 0;
    assert!(open.validate(0).is_err());
    open.columns = 501;
    assert!(open.validate(0).is_err());
    open.columns = 80;
    open.node_session = Uuid::nil();
    assert!(open.validate(0).is_err());
}

#[test]
fn terminal_profile_is_an_identifier_not_an_executable_path() {
    let mut open = request();
    for profile in ["/bin/sh", "../sh", "x;rm", "", "a\0b"] {
        open.profile = profile.into();
        assert!(open.validate(0).is_err(), "拒绝 profile {profile:?}");
    }
}

#[test]
fn terminal_frames_bound_bytes_before_forwarding() {
    let mut frame = TerminalFrame {
        kind: "output".into(),
        data: vec![0; MAX_FRAME_BYTES],
        ..Default::default()
    };
    assert!(validate_frame(&frame).is_ok());
    frame.data.push(0);
    assert!(validate_frame(&frame).is_err());
    frame.data.clear();
    frame.kind = "forward_tcp".into();
    assert!(
        validate_frame(&frame).is_err(),
        "禁止借终端通道转发任意端口"
    );
}
