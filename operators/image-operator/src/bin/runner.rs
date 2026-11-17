//! [INPUT]: 依赖固定任务 Secret 与 Git askpass 环境。
//! [OUTPUT]: 启动单平台镜像构建进程。
//! [POS]: Image Runner 独立进程入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
fn main() -> anyhow::Result<()> {
    if artifactory_image_operator::worker::askpass() {
        return Ok(());
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(artifactory_image_operator::worker::run())
}
