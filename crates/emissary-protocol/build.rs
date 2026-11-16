//! [INPUT]: 依赖 Emissary protobuf 和 Serde 派生。
//! [OUTPUT]: 生成兼容旧持久化记录的 RPC 类型。
//! [POS]: 节点与控制平面的协议生成入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .field_attribute(
            "artifactory.emissary.v1.Assignment.managed_signing_json",
            "#[serde(default)]",
        )
        .field_attribute(
            "artifactory.emissary.v1.Command.signing_session_id",
            "#[serde(default)]",
        )
        .build_transport(false)
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(
            &["../../proto/artifactory/emissary/v1/emissary.proto"],
            &["../../proto"],
        )?;
    println!("cargo:rerun-if-changed=../../proto/artifactory/emissary/v1/emissary.proto");
    Ok(())
}
