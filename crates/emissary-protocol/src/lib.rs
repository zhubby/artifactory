//! [INPUT]: 依赖生成的 gRPC 协议与节点 JSON 契约。
//! [OUTPUT]: 导出节点 RPC、终端、签名、遥测和选机协议。
//! [POS]: 控制平面、原生节点与遥测服务的共享契约边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod nodes;
pub mod signing;
pub mod telemetry;
pub mod terminal;
tonic::include_proto!("artifactory.emissary.v1");
