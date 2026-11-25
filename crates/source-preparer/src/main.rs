//! [INPUT]: 依赖 Pod 身份、任务 URL 和共享检出引擎。
//! [OUTPUT]: 提供轻量解析、引用查询、规划准备和固定检出命令。
//! [POS]: 每次任务的进程入口，不启动网络服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod task;
use clap::{Parser, ValueEnum};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Resolve,
    Refs,
    Checkout,
    Prepare,
}
#[derive(Parser)]
struct Cli {
    #[arg(value_enum)]
    mode: Mode,
}

#[tokio::main]
async fn main() {
    if artifactory_source_checkout::askpass() {
        return;
    }
    let cli = Cli::parse();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut term) => tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}},
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        signal.cancel();
    });
    if let Err(error) = task::run(cli.mode, &cancel).await {
        eprintln!("source preparation failed: {error:#}");
        std::process::exit(1);
    }
}
