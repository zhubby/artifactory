# crates/source-checkout/tests/
> L2 | Parent: ../AGENTS.md

upstream.rs: 验证逐任务拉取、固定身份、LFS、只读目录和索引篡改拒绝。
manifest.rs: 验证摘要、路径和凭据来源边界。
ssh.rs: 隔离 sshd 仅信任夹具工作树及 Git 目录，验证固定提交、密钥和主机信任。
sshd.Dockerfile: 提供 SSH 回归的隔离 Linux daemon，不配置宿主 sshd。

[PROTOCOL]: Update this header on change, then check AGENTS.md
