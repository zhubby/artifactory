//! [INPUT]: 依赖Agent CRD 定义。
//! [OUTPUT]: 输出 Agent 和 Source CRD YAML。
//! [POS]: Agent Operator 的离线 CRD 生成入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> anyhow::Result<()> {
    for crd in artifactory_agent_operator::api::crds() {
        print!("---\n{}", serde_yaml::to_string(&crd)?);
    }
    Ok(())
}
