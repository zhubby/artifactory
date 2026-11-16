# crates/emissary/src/signing/
> L2 | Parent: ../AGENTS.md

mod.rs: 领取固定材料、准备 Keychain，清理后显式解锁，保护清扫并闭锁故障节点。
redact.rs: 在事件持久化前跨日志分块替换本任务秘密。
verify.rs: 原生校验副本、应用和 DMG 票据，以堆缓冲区计算摘要并登记证据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
