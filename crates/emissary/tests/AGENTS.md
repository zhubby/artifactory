# crates/emissary/tests/
> L2 | Parent: ../AGENTS.md

source.rs: 验证 JVM/Maven 缓存隔离、Windows .NET 系统路径、活动目录保护和孤儿会话清理。
contracts.rs: 验证制品边界、签名清单与 OTA 状态机。
release_manifest.rs: 运行发布 CLI，验证统一版本签名与错误标签无写入拒绝。
ota_download.rs: 验证更新下载失败不会替换当前二进制。
live_protocol.rs: 复用隔离租户夹具，验证会话、租约、事件与存储协议。
process_tree.rs: 验证前台与服务入口在配置缺失时退出。
native_service.rs: 隔离测试账户的上游仓库与服务私有目录，验证原生服务生命周期。
runtime.rs: 在线源码准备后验证 EOF、运行中租约和 SIGTERM 回收，并输出提前退出诊断。
terminal_protocol.rs: 通过真实 Emissary、mTLS 和 PTY 验证输入突发、locale、租约与退出码。
support/: 共享平台证书夹具、首连超时诊断与真实租户协议夹具。

[PROTOCOL]: Update this header on change, then check AGENTS.md
