# crates/emissary/src/runner/
> L2 | Parent: ../AGENTS.md

operations.rs: 分离状态和执行授权，先取消投递不执行；关闭等待真实清理和耐久终态。
operations_tests.rs: 验证重连授权、先取消、维护真实计数、已有操作不中断和关闭清理。
command_gate.rs: 按连接能力拒收命令，显式通知只累计进程内饱和计数。
tests.rs: 验证组合心跳、终端拒收、租约及OTA邮箱、健康与排空保存故障恢复。
ota_cache.rs: 后台读取启动器文件与维护标记，缓存进度并记录活动，租约路径不执行文件I/O。
state.rs: 保持构建租约、事件重放及OTA邮箱和健康保存重试，会话围栏保存后才重连。
worker.rs: 监督构建子进程、租约管道与事件，完成源码会话回收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
