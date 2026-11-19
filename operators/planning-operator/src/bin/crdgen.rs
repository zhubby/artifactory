//! [INPUT]: 依赖Planning CRD 定义。
//! [OUTPUT]: 输出 PlanningPool、PlanningRun 与 SourceTask CRD YAML。
//! [POS]: Planning Operator 的离线 CRD 生成入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> anyhow::Result<()> {
    for crd in artifactory_planning_operator::api::crds() {
        print!("---\n{}", serde_yaml::to_string(&crd)?);
    }
    Ok(())
}
