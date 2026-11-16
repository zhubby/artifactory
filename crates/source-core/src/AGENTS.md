# source-core/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 组装源码存储、密钥和缓存目录。
source.rs: 校验仓库协议与凭据边界，供源码服务和控制平面共用。
cache.rs: 固定 Git/LFS 快照，按仓库来源限制凭据复用。
cache/: 查询远端分支和标签。
store/: 持久化源码缓存、租约与快照清单。

[PROTOCOL]: Update this header on change, then check AGENTS.md
