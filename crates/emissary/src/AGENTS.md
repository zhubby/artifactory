# crates/emissary/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出节点能力与统一版本标识。
main.rs: CLI、凭据输入与服务启动入口。
config.rs: 校验节点配置，创建私有目录并原子保存记录。
client.rs: 建立平台 mTLS 通道与节点注册。
runner.rs: 会话、租约、事件确认与工作进程调度。
worker.rs: 执行隔离任务并上传步骤和制品。
signing/: 临时领取材料、清理闭锁并原生校验最终副本。
source.rs: 固定 Git/LFS 快照并校验源码完整性。
process.rs: 约束进程树生命周期与日志流。
artifacts.rs: 在工作目录内收集、读取与校验制品。
doctor.rs: 探测原生工具与节点资源。
ota.rs: 校验签名更新、激活并回滚节点二进制。
service.rs: 安装并控制原生服务。
service/: Windows SCM 服务实现。
runner/: 会话与租约行为回归。
telemetry/: 采集资源、进程与磁盘队列。
bin/: 生成签名节点发布清单。

[PROTOCOL]: Update this header on change, then check AGENTS.md
