// [INPUT]: 依赖生成的 gRPC 协议与节点 JSON 契约。
// [OUTPUT]: 导出节点RPC、命令、遥测协商、清单确认、有限操作与OTA观察契约。
// [POS]: 控制平面、原生节点与遥测服务的共享契约边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod commands;
pub mod inventory;
pub mod nodes;
pub mod operations;
pub mod ota;
pub mod signing;
pub mod telemetry;
pub mod telemetry_negotiation;
pub mod terminal;
tonic::include_proto!("artifactory.emissary.v1");
