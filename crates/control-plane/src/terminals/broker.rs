//! [INPUT]: 依赖已授权的浏览器附着、设备流和持久会话。
//! [OUTPUT]: 单写者转发、附着确认与短断回放，按查询开始截止授权。
//! [POS]: 每个 owner 会话的单任务状态机；全部字节只驻留有界内存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{FrameSender, frame};
use crate::{
    protocol::{
        Command, TerminalFrame,
        terminal::{
            MAX_BUFFER_BYTES, MAX_FRAME_BYTES, OpenTerminal, TerminalExit, TerminalReady,
            validate_frame,
        },
    },
    state::AppState,
    store::terminals::TerminalSession,
};
use serde_json::json;
use std::{collections::VecDeque, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tonic::Status;

pub(super) enum Event {
    Browser {
        sender: FrameSender,
        generation: i64,
        offset: u64,
    },
    BrowserFrame {
        frame: TerminalFrame,
        generation: i64,
    },
    Detached {
        generation: i64,
    },
    Device {
        sender: FrameSender,
        accepted: oneshot::Sender<Result<(), Status>>,
    },
    DeviceFrame(TerminalFrame),
    DeviceDetached,
}

pub(super) async fn run(
    state: AppState,
    session: TerminalSession,
    mut events: mpsc::Receiver<Event>,
) {
    let id = session.id;
    let mut browser: Option<FrameSender> = None;
    let mut browser_cursor = 0u64;
    let mut device: Option<FrameSender> = None;
    let mut device_outbox = VecDeque::<TerminalFrame>::new();
    let mut generation = 0;
    let mut output = VecDeque::<TerminalFrame>::new();
    let mut bytes = 0usize;
    let mut produced = 0u64;
    let mut consumed = 0u64;
    let mut input_sequence = 0u64;
    let mut input_bytes = 0u64;
    let mut account = None;
    let mut last_consumed = Instant::now();
    let mut ready: Option<String> = None;
    let mut opened = false;
    let mut draining: Option<Instant> = None;
    let mut detached = Some(Instant::now());
    let mut auth_deadline = Instant::now() + Duration::from_secs(1);
    let mut next_check = Instant::now();
    let mut next_lease = Instant::now();
    let mut next_open = Instant::now();
    let started = Instant::now();
    let open = OpenTerminal {
        terminal_id: id,
        node_session: session.node_session,
        startup_id: session.startup_id,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 15_000,
        columns: session.columns,
        rows: session.rows,
        term: session.term.clone(),
        profile: session.profile.clone(),
    };
    let open_command = Command {
        command_id: id.to_string(),
        kind: "terminal_open".into(),
        payload_json: serde_json::to_string(&open).unwrap_or_default(),
        ..Default::default()
    };
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut exit = TerminalExit {
        code: None,
        signal: None,
        reason: "session_closed".into(),
    };
    'session: loop {
        if Instant::now() >= auth_deadline {
            exit.reason = "authorization_expired".into();
            break;
        }
        if let Some(sender) = &device {
            while let Some(pending) = device_outbox.front() {
                match sender.try_send(Ok(pending.clone())) {
                    Ok(()) => {
                        device_outbox.pop_front();
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => break,
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        exit.reason = "device_stream_disconnected".into();
                        break 'session;
                    }
                }
            }
        }
        if Instant::now() >= next_check {
            let grant = Instant::now() + Duration::from_secs(5);
            let check =
                tokio::time::timeout_at(auth_deadline, state.store.terminal_authorized(id)).await;
            if Instant::now() >= auth_deadline || !matches!(check, Ok(Ok(true))) {
                exit.reason = "authorization_expired".into();
                break;
            }
            auth_deadline = grant;
            next_check = Instant::now() + Duration::from_secs(2);
        }
        if Instant::now() >= auth_deadline {
            exit.reason = "authorization_expired".into();
            break;
        }
        if let Some(sender) = &browser {
            if output
                .front()
                .is_some_and(|oldest| browser_cursor < oldest.offset)
            {
                exit.reason = "output_gap".into();
                break;
            }
            for _ in 0..32 {
                let Some(saved) = output
                    .iter()
                    .find(|saved| saved.offset + saved.data.len() as u64 > browser_cursor)
                else {
                    break;
                };
                let mut pending = saved.clone();
                if browser_cursor > pending.offset {
                    pending
                        .data
                        .drain(..(browser_cursor - pending.offset) as usize);
                    pending.offset = browser_cursor;
                }
                let end = pending.offset + pending.data.len() as u64;
                match sender.try_send(Ok(pending)) {
                    Ok(()) => browser_cursor = end,
                    Err(mpsc::error::TrySendError::Full(_)) => break,
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        browser = None;
                        detached = Some(Instant::now());
                        break;
                    }
                }
            }
        }
        if draining.is_some()
            && browser_cursor == produced
            && consumed == produced
            && let Some(sender) = &browser
        {
            let mut ended = frame(id, "exit");
            ended.payload_json = serde_json::to_string(&exit).unwrap_or_default();
            match sender.try_send(Ok(ended)) {
                Ok(()) => {
                    browser = None;
                    break;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    browser = None;
                    detached = Some(Instant::now());
                }
            }
        }
        tokio::select! {
            _=state.coordination.stopped()=>{exit.reason="owner_stopped".into();break},
            _=tokio::time::sleep_until(auth_deadline)=>{exit.reason="authorization_expired".into();break},
            _=tick.tick()=>{
                if state.coordination.is_draining(){exit.reason="owner_draining".into();break}
                if draining.is_some_and(|at|at.elapsed()>Duration::from_secs(30)){exit.reason="output_consumer_timeout".into();break}
                if ready.is_none()&&started.elapsed()>Duration::from_secs(15){exit.reason="terminal_start_timeout".into();break}
                if detached.is_some_and(|at|at.elapsed()>Duration::from_secs(30)){exit.reason="browser_disconnected".into();break}
                if produced>consumed && last_consumed.elapsed()>Duration::from_secs(30){exit.reason="output_consumer_timeout".into();break}
                if opened && draining.is_none() && device.is_none() && browser.is_some() && Instant::now()>=next_open {
                    deliver_open(&state,&session,&open_command).await;
                    next_open=Instant::now()+Duration::from_secs(2);
                }
                if Instant::now()>=next_lease && device.is_some() {
                    let mut lease=frame(id,"lease");lease.payload_json=json!({"lease_seconds":45}).to_string();
                    enqueue(&mut device_outbox,lease);
                    next_lease=Instant::now()+Duration::from_secs(10);
                }
            },
            event=events.recv()=>{
                let Some(event)=event else{break};
                if Instant::now()>=auth_deadline {exit.reason="authorization_expired".into();break}
                match event {
                    Event::Browser{sender,generation:next,offset}=>{
                        if next<=generation {let _=sender.try_send(Err(Status::aborted("attachment superseded")));continue}
                        let first=output.front().map(|f|f.offset).unwrap_or(produced);
                        if offset<first||offset>produced {let _=sender.try_send(Err(Status::out_of_range("terminal output gap")));exit.reason="output_gap".into();break}
                        if offset>consumed {
                            consumed=offset;last_consumed=Instant::now();
                            if device.is_some(){let mut ack=frame(id,"ack");ack.offset=offset;enqueue(&mut device_outbox,ack);}
                        }
                        if let Some(previous)=browser.take(){let _=previous.try_send(Err(Status::aborted("attachment superseded")));}
                        generation=next;browser=Some(sender.clone());browser_cursor=offset;detached=None;
                        let mut status=frame(id,if ready.is_some(){"ready"}else{"start"});
                        status.payload_json=ready.clone().unwrap_or_else(||json!({"state":"opening"}).to_string());
                        if sender.try_send(Ok(status)).is_err(){browser=None;detached=Some(Instant::now());continue}
                        if !opened {
                            deliver_open(&state,&session,&open_command).await;
                            let _=tokio::time::timeout_at(auth_deadline,state.store.terminal_state(id,"opening")).await;
                            next_open=Instant::now()+Duration::from_secs(2);
                            opened=true;
                        }
                    },
                    Event::Detached{generation:old}=>if old==generation {browser=None;detached=Some(Instant::now());},
                    Event::BrowserFrame{frame:mut incoming,generation:source}=>{
                        if source!=generation||browser.is_none(){continue}
                        if validate_frame(&incoming).is_err(){exit.reason="invalid_browser_frame".into();break}
                        incoming.terminal_id=id.to_string();
                        match incoming.kind.as_str(){
                            "input"=>{
                                if draining.is_some(){continue}
                                if incoming.offset<=input_sequence{continue}
                                if incoming.data.is_empty(){exit.reason="invalid_input_sequence".into();break}
                                input_sequence=incoming.offset;
                                if ready.is_none(){exit.reason="shell_not_ready".into();break}
                                let length=incoming.data.len() as u64;
                                if !enqueue(&mut device_outbox,incoming){exit.reason="device_congested".into();break}
                                input_bytes+=length;
                                let _=tokio::time::timeout(Duration::from_millis(500),state.store.terminal_input(id)).await;
                            },
                            "resize"=>if draining.is_none(){enqueue(&mut device_outbox,incoming);},
                            "ack"=>{
                                if incoming.offset<consumed||incoming.offset>produced{exit.reason="invalid_output_ack".into();break}
                                if incoming.offset>consumed {last_consumed=Instant::now();}
                                consumed=incoming.offset;
                                if device.is_some(){enqueue(&mut device_outbox,incoming);}
                            },
                            "close"=>{exit.reason="user_closed".into();break},
                            _=>{exit.reason="invalid_browser_frame".into();break}
                        }
                    },
                    Event::Device{sender,accepted}=>{
                        if draining.is_some(){let _=accepted.send(Err(Status::failed_precondition("shell already exited")));continue}
                        if device.is_some(){let _=accepted.send(Err(Status::already_exists("device stream already joined")));continue}
                        let mut start=frame(id,"start");start.columns=session.columns.into();start.rows=session.rows.into();start.payload_json=json!({"profile":session.profile,"term":session.term,"lease_seconds":45}).to_string();
                        if sender.try_send(Ok(start)).is_err(){let _=accepted.send(Err(Status::unavailable("device stream closed")));continue}
                        device=Some(sender);let _=accepted.send(Ok(()));
                    },
                    Event::DeviceFrame(incoming)=>{
                        if draining.is_some(){continue}
                        if validate_frame(&incoming).is_err()||incoming.terminal_id!=id.to_string(){exit.reason="invalid_device_frame".into();break}
                        match incoming.kind.as_str(){
                            "ready"=>{
                                let Ok(details)=serde_json::from_str::<TerminalReady>(&incoming.payload_json)else{exit.reason="invalid_shell_ready".into();break};
                                if ready.is_some(){exit.reason="invalid_shell_ready".into();break}
                                account=Some(details.account);
                                ready=Some(incoming.payload_json.clone());
                                if let Some(tx)=&browser {let _=tx.try_send(Ok(incoming));}
                                let _=tokio::time::timeout_at(auth_deadline,state.store.terminal_state(id,"active")).await;
                            },
                            "output"=>{
                                if ready.is_none()||incoming.data.is_empty()||incoming.data.len()>MAX_FRAME_BYTES||incoming.offset!=produced {exit.reason="invalid_output_sequence".into();break}
                                if produced==consumed {last_consumed=Instant::now();}
                                produced+=incoming.data.len()as u64;
                                if produced.saturating_sub(consumed)>MAX_BUFFER_BYTES as u64 {exit.reason="output_window_exceeded".into();break}
                                bytes+=incoming.data.len();
                                if let Some(last)=output.back_mut() && last.data.len()+incoming.data.len()<=MAX_FRAME_BYTES {
                                    last.data.extend_from_slice(&incoming.data);
                                }else{output.push_back(incoming);}
                                while bytes>MAX_BUFFER_BYTES {
                                    let Some(old)=output.front_mut() else{break};
                                    let remove=(bytes-MAX_BUFFER_BYTES).min(old.data.len());
                                    old.data.drain(..remove);old.offset+=remove as u64;bytes-=remove;
                                    if old.data.is_empty(){output.pop_front();}
                                }
                            },
                            "exit"=>{
                                exit=serde_json::from_str(&incoming.payload_json).unwrap_or_else(|_|TerminalExit{reason:"invalid_exit".into(),..Default::default()});
                                if !["shell_exited","process_exited","cancelled","lease_expired","worker_failed","closed","session_closed"].contains(&exit.reason.as_str()) {exit.reason="device_exit".into();}
                                if exit.signal.as_ref().is_some_and(|signal|signal.len()>64 || !signal.bytes().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(&b))){exit.signal=None;}
                                draining=Some(Instant::now());
                                device=None;
                                device_outbox.clear();
                            },
                            "error"=>{exit.reason="device_error".into();break},
                            _=>{exit.reason="invalid_device_frame".into();break}
                        }
                    },
                    Event::DeviceDetached=>if draining.is_none(){exit.reason="device_stream_disconnected".into();break}
                }
            }
        }
    }
    if let Some(tx) = device {
        let mut close = frame(id, "close");
        close.payload_json = json!({"reason":exit.reason}).to_string();
        let _ = tx.try_send(Ok(close));
    }
    if let Some(connection) = state
        .connections
        .read()
        .await
        .get(&session.node_id)
        .filter(|c| c.session == session.node_session)
    {
        let _ = connection.sender.try_send(Ok(Command {
            kind: "terminal_close".into(),
            payload_json: json!({"terminal_id":id}).to_string(),
            ..Default::default()
        }));
    }
    if let Some(tx) = browser {
        let mut ended = frame(id, "exit");
        ended.payload_json = serde_json::to_string(&exit).unwrap_or_default();
        let _ = tx.try_send(Ok(ended));
    }
    // 已停止转发和续租。数据库恢复后补写终态，避免短暂故障永久占用配额。
    let mut retry = Duration::from_millis(250);
    loop {
        if matches!(
            tokio::time::timeout(
                Duration::from_secs(1),
                state
                    .store
                    .finish_terminal(id, &exit, account.as_deref(), input_bytes, produced)
            )
            .await,
            Ok(Ok(()))
        ) {
            break;
        }
        tokio::select! {_=state.coordination.stopped()=>break,_=tokio::time::sleep(retry)=>{}}
        retry = (retry * 2).min(Duration::from_secs(5));
    }
}

fn enqueue(outbox: &mut VecDeque<TerminalFrame>, frame: TerminalFrame) -> bool {
    if frame.kind == "input" {
        if outbox.iter().filter(|saved| saved.kind == "input").count() >= 32 {
            return false;
        }
    } else if let Some(saved) = outbox.iter_mut().find(|saved| saved.kind == frame.kind) {
        *saved = frame;
        return true;
    }
    outbox.push_back(frame);
    true
}

async fn deliver_open(state: &AppState, session: &TerminalSession, command: &Command) {
    let connection = state
        .connections
        .read()
        .await
        .get(&session.node_id)
        .cloned();
    if let Some(connection) =
        connection.filter(|connection| connection.session == session.node_session)
    {
        // 重送同一 ID 和截止时间，设备以会话 ID 幂等创建 Worker。
        let _ = connection.sender.try_send(Ok(command.clone()));
    }
}
