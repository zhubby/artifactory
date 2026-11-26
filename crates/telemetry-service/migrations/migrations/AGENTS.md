# migrations/
> L2 | Parent: ../AGENTS.md

0000_baseline.sql: 创建采样、聚合、旧清单与幂等回执。
0001_startup_resources.sql: 按节点进程隔离最新资源。
0002_lifecycle.sql: 建立节点与attempt删除围栏。
0003_inventory_delivery.sql: 独立保存清单交付、startup投影与跨副本限流。
0004_inventory_integrity.sql: 验证清单正文复合外键，旧清扫和旧删除遇引用时回滚。

[PROTOCOL]: Update this header on change, then check AGENTS.md
