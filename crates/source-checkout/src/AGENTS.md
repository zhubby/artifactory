# crates/source-checkout/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出清单、临时检出和仓库引用查询。
manifest.rs: 定义唯一源码清单、临时材料和摘要验证。
git.rs: 隔离执行环境并保留 Windows 系统盘和大小写代理，验证索引与认证目录，回收进程树。
checkout.rs: 精确拉取对象、子模块和 LFS，用独立索引验证只读源码。
refs.rs: 查询上游引用；列表保留 annotated tag 对象，构建解析固定同次广告的剥离提交。
diagnostics.rs: 从有界 stderr 生成不含凭据的固定失败分类。
progress.rs: 定义源码阶段、进度序号和安全失败诊断。

[PROTOCOL]: Update this header on change, then check AGENTS.md
