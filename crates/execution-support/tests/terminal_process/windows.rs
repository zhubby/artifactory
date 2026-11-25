//! [INPUT]: 依赖父测试的 Windows 配置、真实 ConPTY 和取消信号。
//! [OUTPUT]: 验证输出消费停顿时取消仍回收 Shell。
//! [POS]: terminal_process 的 Windows 回收回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
async fn windows_stalled_output_does_not_block_cancel() {
    let mut cfg = config();
    cfg.args = vec![
        "/Q".into(),
        "/D".into(),
        "/C".into(),
        "for /L %i in (1,1,1000000) do @echo __PTY_FLOOD__".into(),
    ];
    let (_input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(cfg, rx, tx, cancel.clone()));
    let Some(TerminalEvent::Ready { process_id }) = output.recv().await else {
        panic!("ConPTY did not become ready");
    };
    let mut bytes = Vec::<u8>::new();
    let mut cursor_scan = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&bytes).contains("__PTY_FLOOD__") {
            if let Some(TerminalEvent::Output { data }) = output.recv().await {
                bytes.extend(data);
                while let Some(relative) = bytes[cursor_scan..]
                    .windows(4)
                    .position(|v| v == b"\x1b[6n")
                {
                    cursor_scan += relative + 4;
                    _input
                        .send(TerminalInput::Input {
                            data: b"\x1b[1;1R".to_vec(),
                        })
                        .await
                        .unwrap();
                }
            }
        }
        while output.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("ConPTY did not produce output");
    cancel.cancel();
    let _result = tokio::time::timeout(Duration::from_secs(8), task)
        .await
        .expect("stalled ConPTY prevented cancel")
        .unwrap();
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(process_id)]),
        true,
    );
    assert!(
        system.process(sysinfo::Pid::from_u32(process_id)).is_none(),
        "ConPTY Shell survived cancel"
    );
}
