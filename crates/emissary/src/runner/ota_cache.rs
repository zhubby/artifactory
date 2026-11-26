//! [INPUT]: 固定启动器状态、维护标记与真实排空计数。
//! [OUTPUT]: 后台读取OTA文件，向租约循环提供有限时效缓存。
//! [POS]: Runner的文件I/O边界；心跳与准入只读取内存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use tokio::sync::watch;
type Input = (ota::Drain, bool, Option<uuid::Uuid>);

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub state: Option<ota::State>,
    pub observation: Option<artifactory_emissary_protocol::ota::Observability>,
    pub maintenance: bool,
    pub maintenance_nonce: Option<uuid::Uuid>,
    pub sampled: Option<Instant>,
}
impl Snapshot {
    pub fn fresh(&self) -> bool {
        self.sampled
            .is_some_and(|at| at.elapsed() <= Duration::from_secs(2))
    }
    fn read(
        directory: &std::path::Path,
        drain: ota::Drain,
        report: bool,
        reporter: &mut ota::Reporter,
    ) -> Self {
        let state = ota::snapshot(directory).ok();
        let observation = state.as_ref().filter(|_| report).and_then(|state| {
            reporter
                .observe(directory, state, drain, Instant::now())
                .ok()
                .flatten()
        });
        Self {
            state,
            observation,
            maintenance: ota::maintenance_requested(directory),
            maintenance_nonce: ota::maintenance_nonce(directory).ok().flatten(),
            sampled: Some(Instant::now()),
        }
    }
}

pub(super) fn start(
    directory: Option<PathBuf>,
    jobs: &mut JoinSet<()>,
) -> (watch::Sender<Input>, watch::Receiver<Snapshot>) {
    let (input, receiver) = watch::channel((ota::Drain::default(), false, None));
    let (sender, output) = watch::channel(Snapshot::default());
    if let Some(directory) = directory {
        jobs.spawn(async move {
            let mut reporter = ota::Reporter::default();
            let mut previous = Snapshot::default();
            loop {
                let (drain, report, nonce) = receiver.borrow().clone();
                let path = directory.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let snapshot = Snapshot::read(&path, drain.clone(), report, &mut reporter);
                    let _ = ota::record_fenced_activity(&path, &drain, nonce);
                    (snapshot, reporter)
                })
                .await;
                let Ok((mut snapshot, next_reporter)) = result else {
                    break;
                };
                reporter = next_reporter;
                if snapshot.observation.is_none()
                    && snapshot
                        .state
                        .as_ref()
                        .zip(previous.state.as_ref())
                        .is_some_and(|(new, old)| {
                            new.update_id == old.update_id && new.generation == old.generation
                        })
                {
                    snapshot.observation = previous.observation.take();
                }
                previous = snapshot.clone();
                if sender.send(snapshot).is_err() {
                    break;
                }
                tokio::select! {
                    _=input_closed(&receiver)=>break,
                    _=tokio::time::sleep(Duration::from_secs(1))=>{},
                }
            }
        });
    }
    (input, output)
}
async fn input_closed(receiver: &watch::Receiver<Input>) {
    let mut receiver = receiver.clone();
    while receiver.changed().await.is_ok() {}
}

#[cfg(test)]
pub(super) fn read_fixture(directory: &std::path::Path) -> Snapshot {
    Snapshot::read(
        directory,
        ota::Drain::default(),
        false,
        &mut ota::Reporter::default(),
    )
}
