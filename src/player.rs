use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::sync::mpsc::{Receiver, UnboundedSender};

use crate::app::AppEvent;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("freegal-mpv-{}.sock", std::process::id()))
}

fn header_fields(library_domain: &str) -> String {
    format!("Referer: {library_domain}/,Origin: {library_domain}")
}

pub fn spawn_mpv(socket: &PathBuf, library_domain: &str) -> std::io::Result<Child> {
    let _ = std::fs::remove_file(socket);
    let mut cmd = tokio::process::Command::new("mpv");
    cmd.args([
        "--no-terminal",
        "--idle=yes",
        "--no-config",
        "--no-video",
        "--volume=80",
        format!("--input-ipc-server={}", socket.display()).as_str(),
        format!("--http-header-fields={}", header_fields(library_domain)).as_str(),
    ])
    .env("USER_AGENT", crate::api::client::BROWSER_UA)
    .arg(format!("--user-agent={}", crate::api::client::BROWSER_UA))
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
    cmd.spawn()
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PlayerCommand {
    Load { url: String, title: String },
    InsertNext { url: String, title: String },
    TogglePlay,
    Pause,
    Resume,
    GetStatus,
    Seek { delta: f64 },
    SeekTo { secs: f64 },
    Volume { delta: i8 },
    AudioDevices,
    SetAudioDevice { device: String },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pending {
    Time,
    Duration,
    Pause,
}

fn pending_for(prop: &str) -> Option<Pending> {
    match prop {
        "playback-time" => Some(Pending::Time),
        "duration" => Some(Pending::Duration),
        "pause" => Some(Pending::Pause),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerStatus {
    pub time: Option<f64>,
    pub duration: Option<f64>,
    pub paused: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Status {
        time: Option<f64>,
        duration: Option<f64>,
        paused: Option<bool>,
    },
    EndFile {
        reason: String,
    },
    AudioDevices {
        devices: Vec<(String, String)>,
    },
    BufferingDone,
    Idle,
    Connected,
    ConnectionLost,
    Degraded {
        reason: String,
    },
}

/// One-line, secret-free summary of a player command for logs.
pub fn cmd_label(command: &PlayerCommand) -> String {
    match command {
        PlayerCommand::Load { url, title } => format!(
            "Load{{url[:48]={:?}, title={:?}}}",
            url.chars().take(48).collect::<String>(),
            title.chars().take(40).collect::<String>(),
        ),
        other => format!("{other:?}"),
    }
}

async fn run_connection(
    stream: UnixStream,
    cmd_rx: &mut Receiver<PlayerCommand>,
    events: &UnboundedSender<AppEvent>,
) {
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut req_id: u64 = 0;
    let mut line = String::new();
    loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                let Some(command) = command else {
                    return;
                };
                if matches!(command, PlayerCommand::AudioDevices) {
                    let events = events.clone();
                    tokio::spawn(list_audio_devices(events));
                    continue;
                }
                let is_status = matches!(command, PlayerCommand::GetStatus);
                let request = encode(command, &mut req_id, &mut pending);
                if !is_status {
                    tracing::info!(
                        "mpv ipc → {}",
                        request.trim().chars().take(120).collect::<String>()
                    );
                }
                if wr.write_all(request.as_bytes()).await.is_err() {
                    tracing::warn!("mpv ipc write failed — reconnecting");
                    return;
                }
            }
            read = rd.read_line(&mut line) => {
                match read {
                    Ok(0) => {
                        tracing::warn!("mpv ipc closed by peer — reconnecting");
                        return;
                    }
                    Err(e) => {
                        tracing::warn!("mpv ipc read error {e} — reconnecting");
                        return;
                    }
                    Ok(_) => {
                        handle_line(&line, &mut pending, events);
                        line.clear();
                    }
                }
            }
        }
    }
}

fn encode(command: PlayerCommand, req_id: &mut u64, pending: &mut HashMap<u64, Pending>) -> String {
    let next_id = |req_id: &mut u64| {
        *req_id += 1;
        *req_id
    };
    let requests: Vec<Value> = match command {
        PlayerCommand::Load { url, .. } => vec![json!({
            "command": ["loadfile", url, "replace"],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::InsertNext { url, .. } => vec![json!({
            "command": ["loadfile", url, "insert-next"],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::TogglePlay => vec![json!({
            "command": ["cycle", "pause"],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::Pause => vec![json!({
            "command": ["set", "pause", true],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::Resume => vec![json!({
            "command": ["set", "pause", false],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::GetStatus => ["playback-time", "duration", "pause"]
            .iter()
            .map(|prop| {
                let id = next_id(req_id);
                if let Some(kind) = pending_for(prop) {
                    pending.insert(id, kind);
                }
                json!({ "command": ["get_property", prop], "request_id": id })
            })
            .collect(),
        PlayerCommand::Seek { delta } => vec![json!({
            "command": ["seek", delta, "relative"],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::SeekTo { secs } => vec![json!({
            "command": ["seek", secs, "absolute"],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::Volume { delta } => vec![json!({
            "command": ["add", "volume", delta],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::SetAudioDevice { device } => vec![json!({
            "command": ["set", "audio-device", device],
            "request_id": next_id(req_id),
        })],
        PlayerCommand::AudioDevices | PlayerCommand::Quit => vec![json!({
            "command": ["quit"],
            "request_id": next_id(req_id),
        })],
    };
    requests
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn handle_line(
    line: &str,
    pending: &mut HashMap<u64, Pending>,
    events: &UnboundedSender<AppEvent>,
) {
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        tracing::debug!("mpv non-JSON line: {line}");
        return;
    };
    if let Some(event) = value.get("event").and_then(Value::as_str) {
        let player_event = match event {
            "end-file" => {
                let reason = value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                PlayerEvent::EndFile { reason }
            }
            "playback-restart" => PlayerEvent::BufferingDone,
            "idle" => PlayerEvent::Idle,
            _ => return,
        };
        let _ = events.send(AppEvent::Player(player_event));
        return;
    }
    let Some(request_id) = value.get("request_id").and_then(Value::as_u64) else {
        return;
    };
    let Some(kind) = pending.remove(&request_id) else {
        // Commands like loadfile don't register a pending response; surface
        // mpv's rejections (e.g. "invalid parameter") instead of dropping them.
        let err = value.get("error").and_then(Value::as_str);
        if err.is_some_and(|e| e != "success") {
            tracing::warn!("mpv rejected ipc request {request_id}: {err:?}");
        }
        return;
    };
    let data = value.get("data");
    let player_event = match kind {
        Pending::Time => PlayerEvent::Status {
            time: data.and_then(Value::as_f64),
            duration: None,
            paused: None,
        },
        Pending::Duration => PlayerEvent::Status {
            time: None,
            duration: data.and_then(Value::as_f64),
            paused: None,
        },
        Pending::Pause => PlayerEvent::Status {
            time: None,
            duration: None,
            paused: data.and_then(Value::as_bool),
        },
    };
    let _ = events.send(AppEvent::Player(player_event));
}

pub async fn run_ipc(
    mut cmd_rx: Receiver<PlayerCommand>,
    events: UnboundedSender<AppEvent>,
    socket: PathBuf,
) {
    let mut first = true;
    loop {
        match connect(&socket).await {
            None => {
                let _ = events.send(AppEvent::Player(PlayerEvent::Degraded {
                    reason: " mpv IPC socket unreachable".into(),
                }));
                tokio::time::sleep(backoff(first)).await;
                first = false;
            }
            Some(stream) => {
                let _ = events.send(AppEvent::Player(PlayerEvent::Connected));
                run_connection(stream, &mut cmd_rx, &events).await;
                if channel_closed(&mut cmd_rx) {
                    return;
                }
                let _ = events.send(AppEvent::Player(PlayerEvent::ConnectionLost));
                tokio::time::sleep(backoff(first)).await;
                first = false;
            }
        }
    }
}

fn channel_closed(rx: &mut Receiver<PlayerCommand>) -> bool {
    rx.try_recv()
        .is_err_and(|e| e == tokio::sync::mpsc::error::TryRecvError::Disconnected)
}

async fn connect(socket: &PathBuf) -> Option<UnixStream> {
    for attempt in 0..40u32 {
        if let Ok(stream) = UnixStream::connect(socket).await {
            return Some(stream);
        }
        tokio::time::sleep(Duration::from_millis(if attempt < 20 { 50 } else { 250 })).await;
    }
    None
}

pub async fn run(
    mut cmd_rx: Receiver<PlayerCommand>,
    events: UnboundedSender<AppEvent>,
    socket: PathBuf,
    library_domain: String,
) {
    match spawn_mpv(&socket, &library_domain) {
        Ok(mut child) => {
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(drain_stderr(stderr));
            }
            run_ipc(cmd_rx, events, socket).await;
            let _ = child.kill().await;
        }
        Err(e) => {
            let _ = events.send(AppEvent::Player(PlayerEvent::Degraded {
                reason: format!(" spawn failed: {e}"),
            }));
            while cmd_rx.recv().await.is_some() {}
        }
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr) {
    let mut rd = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        match rd.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                tracing::debug!("mpv: {}", line.trim_end());
                line.clear();
            }
        }
    }
}

fn backoff(first: bool) -> Duration {
    if first {
        Duration::from_millis(50)
    } else {
        Duration::from_secs(15)
    }
}

async fn list_audio_devices(events: UnboundedSender<AppEvent>) {
    let output = tokio::process::Command::new("mpv")
        .arg("--audio-device=help")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await;
    let devices = output
        .map(|out| parse_device_list(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or_default();
    let _ = events.send(AppEvent::Player(PlayerEvent::AudioDevices { devices }));
}

fn parse_device_list(text: &str) -> Vec<(String, String)> {
    let mut devices = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix('\'') else {
            continue;
        };
        let Some((id, tail)) = rest.split_once('\'') else {
            continue;
        };
        let desc = tail
            .find(" (")
            .map(|at| {
                let desc = &tail[at + 2..];
                desc.strip_suffix(')').unwrap_or(desc)
            })
            .unwrap_or_default();
        devices.push((id.to_string(), desc.to_string()));
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppEvent;
    use std::time::Duration;
    use tempfile::tempdir;

    fn fake_server(
        path: PathBuf,
        drain_requests: usize,
        scripted: Vec<String>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let listener = match tokio::net::UnixListener::bind(&path) {
                Ok(l) => l,
                Err(_) => {
                    return;
                }
            };
            let (stream, _) = listener.accept().await.expect("accept");
            let (rd, mut wr) = stream.into_split();
            let mut rd = BufReader::new(rd);
            let mut line = String::new();
            for _ in 0..drain_requests {
                let _ = rd.read_line(&mut line).await;
                line.clear();
            }
            for message in &scripted {
                wr.write_all(format!("{message}\n").as_bytes())
                    .await
                    .expect("write scripted line");
            }
            loop {
                match rd.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => line.clear(),
                }
            }
        })
    }

    #[test]
    fn parse_device_list_sample() {
        let text = "List of detected audio devices:\n  'auto' (Autoselect device)\n  'pipewire/bluez_output.68_F2_1F_3E_35_1D.1' (Obsidian 2)\n  'alsa/pipewire' (PipeWire Sound Server)\n";
        assert_eq!(
            parse_device_list(text),
            vec![
                ("auto".to_string(), "Autoselect device".to_string()),
                (
                    "pipewire/bluez_output.68_F2_1F_3E_35_1D.1".to_string(),
                    "Obsidian 2".to_string()
                ),
                (
                    "alsa/pipewire".to_string(),
                    "PipeWire Sound Server".to_string()
                ),
            ]
        );
        assert!(parse_device_list("").is_empty());
    }

    #[tokio::test]
    async fn status_poll_demux_interleaved() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("t.sock");
        let server = fake_server(
            socket.clone(),
            3,
            vec![
                r#"{"data":214.0,"error":"success","request_id":2}"#.into(),
                r#"{"data":false,"error":"success","request_id":3}"#.into(),
                r#"{"event":"playback-restart"}"#.into(),
                r#"{"data":61.5,"error":"success","request_id":1}"#.into(),
            ],
        );
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(run_ipc(cmd_rx, ev_tx, socket.clone()));
        cmd_tx.send(PlayerCommand::GetStatus).await.unwrap();

        let mut got = Vec::new();
        for _ in 0..5 {
            let ev = tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                .await
                .unwrap();
            got.push(ev);
        }
        assert!(got
            .iter()
            .any(|e| matches!(e, Some(AppEvent::Player(PlayerEvent::Connected)))));
        let statuses: Vec<PlayerEvent> = got
            .into_iter()
            .flatten()
            .filter_map(|e| match e {
                AppEvent::Player(p) => Some(p),
                _ => None,
            })
            .filter(|p| !matches!(p, PlayerEvent::Connected))
            .collect();
        assert_eq!(
            statuses.len(),
            4,
            "3 status partials + BufferingDone, got {statuses:?}"
        );
        assert!(matches!(
            statuses[0],
            PlayerEvent::Status {
                duration: Some(_),
                ..
            }
        ));
        assert!(matches!(
            statuses[1],
            PlayerEvent::Status {
                paused: Some(false),
                ..
            }
        ));
        assert!(matches!(statuses[2], PlayerEvent::BufferingDone));
        assert!(matches!(
            statuses[3],
            PlayerEvent::Status { time: Some(t), .. } if (t - 61.5).abs() < f64::EPSILON
        ));
        drop(cmd_tx);
        handle.await.unwrap();
        let _ = server.await;
    }

    #[tokio::test]
    async fn end_file_and_idle_events() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("t2.sock");
        let server = fake_server(
            socket.clone(),
            1,
            vec![
                r#"{"error":"success","request_id":1}"#.into(),
                r#"{"event":"end-file","reason":"eof"}"#.into(),
                r#"{"event":"idle"}"#.into(),
            ],
        );
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);
        let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(run_ipc(cmd_rx, ev_tx, socket.clone()));
        cmd_tx.send(PlayerCommand::TogglePlay).await.unwrap();
        let mut eof = false;
        let mut idle = false;
        for _ in 0..5 {
            let ev = tokio::time::timeout(Duration::from_secs(2), ev_rx.recv())
                .await
                .unwrap();
            match ev {
                Some(AppEvent::Player(PlayerEvent::EndFile { reason })) if reason == "eof" => {
                    eof = true
                }
                Some(AppEvent::Player(PlayerEvent::Idle)) => idle = true,
                _ => {}
            }
            if eof && idle {
                break;
            }
        }
        assert!(eof && idle);
        drop(cmd_tx);
        handle.await.unwrap();
        let _ = server.await;
    }
}
