# crates/source-checkout/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出清单、临时检出和仓库引用查询。
manifest.rs: 定义唯一源码清单、临时材料和摘要验证。
git.rs: 隔离 Git 环境、验证索引和认证目录，回收进程树。
checkout.rs: 精确拉取对象、子模块和 LFS，用独立索引验证只读源码。
refs.rs: 查询上游引用，保留 annotated tag 对象身份。

[PROTOCOL]: Update this header on change, then check AGENTS.md
