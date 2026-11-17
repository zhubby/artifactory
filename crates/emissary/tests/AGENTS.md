# crates/emissary/tests/
> L2 | Parent: ../AGENTS.md

source.rs: 验证 Git 缓存隔离、源码校验与 LFS 副本。
contracts.rs: 验证制品边界、签名清单与 OTA 状态机。
ota_download.rs: 验证更新下载失败不会替换当前二进制。
live_protocol.rs: 复用隔离租户夹具，验证会话、租约、事件与存储协议。
process_tree.rs: 验证取消与子进程回收，PID 标记原子发布。
native_service.rs: 在显式启用的宿主验证原生服务生命周期。
runtime.rs: 验证前台运行、OTA 和双向 TLS，暴露首连前的监督错误。
support/: 共享平台证书夹具、首连超时诊断与真实租户协议夹具。

[PROTOCOL]: Update this header on change, then check AGENTS.md
