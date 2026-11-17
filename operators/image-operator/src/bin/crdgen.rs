//! [INPUT]: 依赖 Image Operator CRD 定义。
//! [OUTPUT]: 输出独立 Image CRD YAML。
//! [POS]: Image Operator 的离线 CRD 生成入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> anyhow::Result<()> {
    for crd in artifactory_image_operator::api::crds() {
        print!("---\n{}", serde_yaml::to_string(&crd)?);
    }
    Ok(())
}
