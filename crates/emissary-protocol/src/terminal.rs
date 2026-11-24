//! [INPUT]: 依赖 UUID、Serde 和生成的终端流信封。
//! [OUTPUT]: 提供终端启动身份、设备能力、退出信息和有界帧校验。
//! [POS]: 设备、会话服务和内部流共用的终端契约，不包含秘密日志。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::TerminalFrame;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 启用设备终端时上报的独立能力。
pub const CAPABILITY: &str = "terminal-v1";
/// 单个输入或输出数据帧最多 16 KiB。
pub const MAX_FRAME_BYTES: usize = 16 * 1024;
/// 每会话的在途字节或环形输出最多 1 MiB。
pub const MAX_BUFFER_BYTES: usize = 1024 * 1024;
/// 启动与内部认证元数据的独立预算。
pub const MAX_CONTROL_BYTES: usize = 8 * 1024;
/// 设备在失去会话续期后回收 Worker 的最长等待。
pub const LEASE_SECONDS: u64 = 45;
/// 页面保留原渲染器时可恢复的短断窗口。
pub const RECONNECT_SECONDS: u64 = 30;

/// 固定设备启动和服务端控制连接的终端启动描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenTerminal {
    pub terminal_id: Uuid,
    pub node_session: Uuid,
    pub startup_id: Uuid,
    pub expires_at_ms: i64,
    pub columns: u16,
    pub rows: u16,
    pub term: String,
    pub profile: String,
}

impl OpenTerminal {
    /// 校验启动身份、截止时间和由设备选择的 profile 标识。
    ///
    /// # Errors
    /// 拒绝过期、空身份、任意路径和超出窗口预算的请求。
    pub fn validate(&self, now_ms: i64) -> Result<()> {
        ensure!(
            !self.terminal_id.is_nil() && !self.node_session.is_nil() && !self.startup_id.is_nil(),
            "invalid terminal identity"
        );
        ensure!(
            self.expires_at_ms > now_ms && self.expires_at_ms.saturating_sub(now_ms) <= 30_000,
            "terminal open expired"
        );
        validate_size(self.columns.into(), self.rows.into())?;
        ensure!(identifier(&self.profile), "invalid terminal profile");
        ensure!(
            !self.term.is_empty()
                && self.term.len() <= 64
                && self
                    .term
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
            "invalid terminal type"
        );
        Ok(())
    }
}

/// 设备端配置暴露的 profile 标识，不暴露任意可执行路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalProfile {
    pub id: String,
    pub label: String,
}

/// 设备启动元数据中的终端能力，命名空间为 emissary.terminal。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalCapabilities {
    pub enabled: bool,
    pub profiles: Vec<TerminalProfile>,
    pub account: String,
}

/// 独立设备终端流首帧中的身份；实际节点仍由 mTLS 证书确认。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalHello {
    pub node_id: Uuid,
    pub node_session: Uuid,
    pub startup_id: Uuid,
}

/// 真 PTY 创建成功后的运行信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalReady {
    pub account: String,
    pub cwd: String,
    pub shell: String,
}

/// 实际退出信息；网络错误不伪造进程退出码。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalExit {
    pub code: Option<i32>,
    pub signal: Option<String>,
    pub reason: String,
}

/// 窗口尺寸以字符计算，限制内核和渲染器的分配规模。
///
/// # Errors
/// 拒绝零尺寸或大于 500 行列的窗口。
pub fn validate_size(columns: u32, rows: u32) -> Result<()> {
    ensure!(
        (1..=500).contains(&columns) && (1..=500).contains(&rows),
        "invalid terminal size"
    );
    Ok(())
}

/// 在解码控制 JSON 和转发数据前检查独立流的信封预算。
///
/// # Errors
/// 拒绝未知消息、超额字节、偏移溢出和非法 resize。
pub fn validate_frame(frame: &TerminalFrame) -> Result<()> {
    ensure!(
        frame.data.len() <= MAX_FRAME_BYTES,
        "terminal frame exceeds byte limit"
    );
    ensure!(
        frame.payload_json.len() <= MAX_CONTROL_BYTES,
        "terminal control exceeds byte limit"
    );
    ensure!(
        [
            "hello",
            "start",
            "ready",
            "input",
            "output",
            "resize",
            "ack",
            "lease",
            "close",
            "exit",
            "error",
            "relay_hello"
        ]
        .contains(&frame.kind.as_str()),
        "unknown terminal frame"
    );
    ensure!(
        frame.data.is_empty() || matches!(frame.kind.as_str(), "input" | "output"),
        "unexpected terminal bytes"
    );
    ensure!(
        frame.offset.checked_add(frame.data.len() as u64).is_some(),
        "terminal offset overflow"
    );
    if frame.kind == "resize" {
        validate_size(frame.columns, frame.rows)?;
    }
    Ok(())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
