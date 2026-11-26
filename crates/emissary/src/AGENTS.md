# crates/emissary/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出节点能力与统一版本标识。
main.rs: CLI、注册名称规范化、凭据输入、服务启动与固定启动器维护恢复入口。
config.rs: 校验节点配置，复用私有文件原语并原子保存记录。
cleanup.rs: 标准删除受管目录；Windows 受限缓存链接只删除链接，保留目标和 ACL。
client.rs: 校验入口地址 SAN，建立平台 mTLS 通道与节点注册。
runner.rs: 会话、通知准入、缓存心跳、OTA保存故障重试、租约和独立进程调度。
worker.rs: 执行隔离步骤；取消时回收后代并有界关闭日志，再上传制品或清理目录。
signing/: 临时领取材料、清理闭锁并原生校验最终副本。
source.rs: 领取源码材料，隔离缓存，回收会话后通过 cleanup 清理任务目录。
process.rs: 按出生时间登记Worker与PTY；接受Darwin退出过渡态并回收后代。
artifacts.rs: 在工作目录内收集、读取与校验制品。
operations/: 固定只读探测、耐久接受与终态日志，支持断线重传和取消。
doctor.rs: 八路并发探测原生工具，按名称固定首连能力快照，并保留每工具超时。
ota.rs: 耐久接受签名更新并监督激活；保存故障保全进程，候选失败先恢复原程序。
ota/: 实现有界下载进度、空间预检、受保护GC和严格运行期续传。
service.rs: 安装并控制原生服务，导出保留身份的固定启动器维护入口。
service/: Windows SCM服务与保留原身份、服务定义和备份的启动器维护事务。
runner/: 会话与租约行为回归。
telemetry/: 固定主机启动元数据，缓存本地接口诊断并持久化严格遥测批次。
terminal/: 校验本地启用、独立终端流、租约与 PTY Worker，隔离构建控制。
bin/: 生成签名节点发布清单。

[PROTOCOL]: Update this header on change, then check AGENTS.md
