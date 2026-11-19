# service-transport/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出独立于规划契约的通信基础设施。
crypto.rs: 密封凭据，生成随机令牌和稳定摘要。
error.rs: 将身份拒绝和服务故障转换为 HTTP 状态。
metrics.rs: 统计服务请求，禁止业务身份进入指标标签。
proxy.rs: 校验池级出口和内部直连清单，生成标准代理环境。
service.rs: 校验端点，限制 JSON 大小，提供认证客户端与 TLS 监听。

[PROTOCOL]: Update this header on change, then check AGENTS.md
