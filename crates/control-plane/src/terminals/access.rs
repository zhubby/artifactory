//! [INPUT]: 依赖当前会话读取权限、停止信号与单调时钟。
//! [OUTPUT]: 从查询开始计五秒授权截止，取消流和有界等待转发。
//! [POS]: 每个公开及内部出口均丢弃撤权后的积压，不等待队尾退出帧。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Frames;
use crate::{protocol::TerminalFrame, state::AppState};
use futures::Stream;
use std::{pin::Pin, time::Duration};
use tokio::{sync::watch, time::Instant};
use tokio_util::sync::CancellationToken;
use tonic::Status;
use uuid::Uuid;

pub type AuthorizedFrames = Pin<Box<dyn Stream<Item = Result<TerminalFrame, Status>> + Send>>;

pub(crate) struct Access {
    pub(crate) cancelled: CancellationToken,
    pub(crate) deadline: watch::Receiver<Instant>,
    stop: CancellationToken,
}

impl Access {
    pub(crate) async fn new(state: AppState, id: Uuid) -> Result<Self, Status> {
        let initial = Instant::now() + Duration::from_secs(5);
        if !matches!(
            tokio::time::timeout(
                Duration::from_secs(1),
                state.store.terminal_access_valid(id)
            )
            .await,
            Ok(Ok(true))
        ) || Instant::now() >= initial
        {
            return Err(Status::permission_denied("terminal authorization expired"));
        }
        let (updates, deadline) = watch::channel(initial);
        let cancelled = CancellationToken::new();
        let failed = cancelled.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            let mut expires = initial;
            loop {
                tokio::select! {biased;_=stopped.cancelled()=>break,_=state.coordination.stopped()=>{failed.cancel();break},_=tokio::time::sleep_until(expires)=>{failed.cancel();break},_=tokio::time::sleep(Duration::from_secs(2))=>{}}
                let grant = Instant::now() + Duration::from_secs(5);
                let result = tokio::select! {biased;_=stopped.cancelled()=>break,_=tokio::time::sleep_until(expires)=>{failed.cancel();break},result=state.store.terminal_access_valid(id)=>result};
                if Instant::now() >= expires || !matches!(result, Ok(true)) {
                    failed.cancel();
                    break;
                }
                expires = grant;
                if updates.send(expires).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            cancelled,
            deadline,
            stop,
        })
    }

    pub(crate) fn valid(&self) -> bool {
        !self.cancelled.is_cancelled() && Instant::now() < *self.deadline.borrow()
    }

    pub(crate) async fn run<T>(
        &mut self,
        future: impl std::future::Future<Output = T>,
        timeout: Duration,
    ) -> Result<T, Status> {
        tokio::pin!(future);
        let operation_deadline = Instant::now() + timeout;
        loop {
            if !self.valid() {
                return Err(Status::permission_denied("terminal authorization expired"));
            }
            let deadline = *self.deadline.borrow();
            tokio::select! {biased;
                _=self.cancelled.cancelled()=>return Err(Status::permission_denied("terminal authorization expired")),
                changed=self.deadline.changed()=>{if changed.is_err(){return Err(Status::permission_denied("terminal authorization expired"))}},
                _=tokio::time::sleep_until(deadline)=>{if !self.valid(){return Err(Status::permission_denied("terminal authorization expired"))}},
                _=tokio::time::sleep_until(operation_deadline)=>return Err(Status::deadline_exceeded("terminal forwarding congested")),
                result=&mut future=>return Ok(result),
            }
        }
    }

    pub(crate) fn stream(mut self, mut frames: Frames) -> AuthorizedFrames {
        Box::pin(async_stream::stream! {
            loop {
                if !self.valid(){yield Err(Status::permission_denied("terminal authorization expired"));break}
                let deadline=*self.deadline.borrow();
                tokio::select!{biased;
                    _=self.cancelled.cancelled()=>{yield Err(Status::permission_denied("terminal authorization expired"));break},
                    changed=self.deadline.changed()=>{if changed.is_err(){break}},
                    _=tokio::time::sleep_until(deadline)=>{yield Err(Status::permission_denied("terminal authorization expired"));break},
                    value=frames.recv()=>match value {Some(value)=>{if !self.valid(){yield Err(Status::permission_denied("terminal authorization expired"));break}yield value},None=>break}
                }
            }
        })
    }
}

impl Drop for Access {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
