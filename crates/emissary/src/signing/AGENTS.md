# crates/emissary/src/signing/
> L2 | Parent: ../AGENTS.md

mod.rs: 领取固定材料、准备 Keychain，清理后显式解锁，保护清扫并闭锁故障节点。
redact.rs: 在事件持久化前跨日志分块替换本任务秘密。
verify.rs: 提取原生证书链，校验副本、应用和 DMG 票据，计算摘要并登记证据。
native_macos_tests.rs: 读取系统应用，回归真实证书链提取和错误指纹拒绝。

[PROTOCOL]: Update this header on change, then check AGENTS.md
