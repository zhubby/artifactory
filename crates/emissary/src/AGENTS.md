# crates/emissary/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出节点能力与统一版本标识。
main.rs: CLI、注册名称规范化、凭据输入与服务启动入口。
config.rs: 校验节点配置，复用私有文件原语并原子保存记录。
client.rs: 校验入口地址 SAN，建立平台 mTLS 通道与节点注册。
runner.rs: 会话、固定主机名与接口缓存心跳、租约和独立进程调度。
worker.rs: 复用共享事件管道执行隔离任务，上传制品并验证 Worker 栈预算。
signing/: 临时领取材料、清理闭锁并原生校验最终副本。
source.rs: 领取源码材料，隔离 JVM/Maven 等任务缓存并在删除前回收会话。
process.rs: 按出生时间登记 Worker 与 PTY；保留 Darwin 僵尸身份并回收后代。
artifacts.rs: 在工作目录内收集、读取与校验制品。
doctor.rs: 八路并发探测原生工具，按名称固定首连能力快照，并保留每工具超时。
ota.rs: 校验签名更新、激活并回滚节点二进制。
service.rs: 安装并控制原生服务。
service/: Windows SCM 服务实现。
runner/: 会话与租约行为回归。
telemetry/: 固定主机启动元数据，缓存本地接口诊断并持久化严格遥测批次。
terminal/: 校验本地启用、独立终端流、租约与 PTY Worker，隔离构建控制。
bin/: 生成签名节点发布清单。

[PROTOCOL]: Update this header on change, then check AGENTS.md
