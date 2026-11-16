//! [INPUT]: 依赖生成的 gRPC 协议与签名 JSON 契约。
//! [OUTPUT]: 导出设备、规划及托管签名协议。
//! [POS]: 控制平面与执行节点的共享传输边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod signing;
tonic::include_proto!("artifactory.emissary.v1");
