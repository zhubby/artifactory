//! [INPUT]: 真实队列准备器、遥测Handle、固定配置与mTLS传输门。
//! [OUTPUT]: 准备或读取冻结源集合，固定Fresh身份并核对ACK前后文件。
//! [POS]: Runner、原生维护与磁盘恢复共用的数据夹具；不构造成功回执。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![allow(dead_code, reason = "共享夹具由独立集成测试按需调用")]
use crate::{queues, telemetry_gate::TelemetryGate};
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{config::Config, telemetry};
use artifactory_emissary_protocol::{
    InventoryObservation,
    telemetry::{Batch, Inventory},
};
use prost::Message;
use std::{collections::BTreeSet, fs, path::PathBuf, time::Duration};
use uuid::Uuid;

pub struct Sources {
    pub files: Vec<(PathBuf, Vec<u8>)>,
    pub batches: Vec<(PathBuf, Vec<u8>, Batch)>,
    pub inventories: Vec<(PathBuf, Vec<u8>, InventoryObservation)>,
    pub startups: BTreeSet<Uuid>,
    pub inventory: Inventory,
    pub fresh_id: Uuid,
    pub journal: (PathBuf, Vec<u8>),
}
impl Sources {
    pub async fn prepare(cfg: &Config) -> Result<Self> {
        let mut preparation = cfg.clone();
        preparation.telemetry_endpoint.clear();
        let files = queues::prepare(&preparation).await?;
        Self::from_files(files)
    }

    pub fn from_files(files: Vec<(PathBuf, Vec<u8>)>) -> Result<Self> {
        let mut batches = Vec::new();
        let mut inventories = Vec::new();
        let mut startups = BTreeSet::new();
        let mut fresh_id = None;
        let mut journal = None;
        let mut inventory = None;
        for (path, bytes) in &files {
            match queues::validate(path, bytes)? {
                kind @ ("ordinary_batch" | "history_batch") => {
                    let batch: Batch = serde_json::from_slice(bytes)?;
                    startups.insert(batch.session_id);
                    if kind == "ordinary_batch" {
                        fresh_id = Some(batch.id);
                    }
                    batches.push((path.clone(), bytes.clone(), batch));
                }
                "startup_inventory" | "refresh_inventory" | "legacy_inventory" => {
                    let observation = InventoryObservation::decode(bytes.as_slice())?;
                    startups.insert(Uuid::parse_str(&observation.source_startup_id)?);
                    if observation.source_batch_id.is_empty() && observation.revision == 0 {
                        inventory = Some(observation.validate()?);
                    }
                    inventories.push((path.clone(), bytes.clone(), observation));
                }
                "terminal_operation" => journal = Some((path.clone(), bytes.clone())),
                _ => anyhow::bail!("unexpected source category"),
            }
        }
        ensure!(
            batches.len() == 2 && inventories.len() == 3,
            "source matrix changed"
        );
        Ok(Self {
            files,
            batches,
            inventories,
            startups,
            fresh_id: fresh_id.context("source fresh sample missing")?,
            inventory: inventory.context("source startup inventory missing")?,
            journal: journal.context("source terminal journal missing")?,
        })
    }

    /// 真实发送器选择原Fresh；关闭并释放队列后才更新文件位置。
    pub async fn pin(&mut self, cfg: &Config, gate: &TelemetryGate) -> Result<()> {
        let original_startup = self
            .batches
            .iter()
            .find(|(_, _, batch)| batch.id == self.fresh_id)
            .context("fresh startup missing")?
            .2
            .session_id;
        let sender =
            telemetry::Handle::start(cfg.clone(), original_startup, self.inventory.clone());
        let selected = tokio::time::timeout(Duration::from_secs(15), async {
            while !gate.saw_batch(self.fresh_id)? || !gate.saw_inventory()? {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("real sender did not select original pending records")
        .and_then(|result| result);
        // 超时与观察失败也先等待真实任务回收，再传播原错误。
        let shutdown = sender.shutdown().await;
        selected?;
        shutdown.context("real source sender did not release queue locks")?;
        self.locate_fresh(cfg)?;
        self.retained()
    }

    pub fn locate_fresh(&mut self, cfg: &Config) -> Result<()> {
        let actual = cfg
            .root
            .join("telemetry/fresh")
            .join(format!("{}.batch", self.fresh_id));
        let expected = self
            .batches
            .iter_mut()
            .find(|(_, _, batch)| batch.id == self.fresh_id)
            .context("source fresh sample missing")?;
        queues::verify(&actual, &expected.1, &fs::read(&actual)?)?;
        let previous = expected.0.clone();
        expected.0 = actual.clone();
        self.files
            .iter_mut()
            .find(|(path, _)| *path == previous)
            .context("source fresh file missing")?
            .0 = actual;
        Ok(())
    }

    pub fn retained(&self) -> Result<()> {
        for (path, expected) in &self.files {
            queues::verify(
                path,
                expected,
                &fs::read(path).context("unacknowledged source missing")?,
            )?;
        }
        Ok(())
    }

    pub fn removed(&self) -> Result<bool> {
        for path in self
            .batches
            .iter()
            .map(|(path, _, _)| path)
            .chain(self.inventories.iter().map(|(path, _, _)| path))
        {
            if path.try_exists()? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn journal_retained(&self) -> Result<()> {
        queues::verify(
            &self.journal.0,
            &self.journal.1,
            &fs::read(&self.journal.0)?,
        )
    }
}
