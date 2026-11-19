# crates/emissary/src/signing/
> L2 | Parent: ../AGENTS.md

mod.rs: 领取固定材料、准备 Keychain，清理后显式解锁；复用共享脱敏器。
verify.rs: 提取原生证书链，校验副本、应用和 DMG 票据，计算摘要并登记证据。
native_macos_tests.rs: 固定系统夹具证书链有效期，回归原生提取、信任和错误指纹拒绝。

[PROTOCOL]: Update this header on change, then check AGENTS.md
