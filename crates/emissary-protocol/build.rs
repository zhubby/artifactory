//! [INPUT]: 依赖 Emissary protobuf 和 Serde 派生。
//! [OUTPUT]: 生成严格节点任务与临时材料 RPC 类型。
//! [POS]: 节点与控制平面的协议生成入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_transport(false)
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(
            &["../../proto/artifactory/emissary/v1/emissary.proto"],
            &["../../proto"],
        )?;
    println!("cargo:rerun-if-changed=../../proto/artifactory/emissary/v1/emissary.proto");
    Ok(())
}
