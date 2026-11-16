# crates/control-plane/src/signing/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合托管签名契约、加密、导入与计划约束。
encryption.rs: 以独立密钥环执行带租户版本上下文的信封加密。
import.rs: 解密 PKCS12/JKS 并核对私钥、证书和用途。
policy.rs: 校验默认方案、应用平台和最终验签输出覆盖。

[PROTOCOL]: Update this header on change, then check AGENTS.md
