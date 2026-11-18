//! ```text
//! [INPUT]: 依赖最小本地协议、Linux 进程监督与管理员 deadline。
//! [OUTPUT]: 启动无业务凭据的 PID 1，失败只输出有界 I/O 分类。
//! [POS]: 可独立静态链接到官方 rootless 镜像的监督入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
#[cfg(target_os = "linux")]
#[path = "../buildkit.rs"]
mod buildkit;
#[cfg(target_os = "linux")]
use artifactory_image_operator::lifecycle;

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(buildkit::run());
        if let Err(error) = result {
            let kind = error
                .downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind);
            let message = format!("BuildKit supervisor failed; io_kind={kind:?}");
            let _ = std::fs::write("/dev/termination-log", &message);
            eprintln!("{message}");
            std::process::exit(1);
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("BuildKit supervisor requires Linux")
}
