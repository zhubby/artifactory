//! [INPUT]: 依赖发布专属 Secret 与已验证镜像结果。
//! [OUTPUT]: 启动镜像发布进程。
//! [POS]: Image Publisher 独立进程入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    artifactory_image_operator::publisher::run().await
}
