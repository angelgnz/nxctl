//! Line-delimited JSON IPC protocol between `nxctl` and `nxctld` over a Unix socket.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(3);

/// Request sent from the client to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    Reload,
    SetProfile { profile: String },
}

/// Snapshot of daemon state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub active_profile: String,
    pub config_path: Option<String>,
    pub poll_interval_secs: u64,
    pub hysteresis_celsius: f32,
    pub min_duty_delta: u8,
}

/// Response sent from the daemon to the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok { message: String },
    Status { daemon: DaemonStatus },
    Error { message: String },
}

/// Default daemon socket path (`$XDG_RUNTIME_DIR/nxctld.sock`, or `/tmp/nxctld.sock`).
pub fn default_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .map(|d| d.join("nxctld.sock"))
        .unwrap_or_else(|| PathBuf::from("/tmp/nxctld.sock"))
}

/// Send one request to the daemon and read one response line.
pub fn send_request(socket_path: &Path, request: &IpcRequest) -> crate::Result<IpcResponse> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    Ok(serde_json::from_str(buf.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn status() -> DaemonStatus {
        DaemonStatus {
            active_profile: "quiet".into(),
            config_path: Some("/etc/nxctl.toml".into()),
            poll_interval_secs: 2,
            hysteresis_celsius: 1.5,
            min_duty_delta: 3,
        }
    }

    #[test]
    fn request_roundtrip() {
        for r in [
            IpcRequest::Status,
            IpcRequest::Reload,
            IpcRequest::SetProfile {
                profile: "x".into(),
            },
        ] {
            let s = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<IpcRequest>(&s).unwrap(), r);
        }
        assert_eq!(
            serde_json::to_string(&IpcRequest::Status).unwrap(),
            r#"{"cmd":"status"}"#
        );
    }

    #[test]
    fn response_roundtrip() {
        for r in [
            IpcResponse::Ok {
                message: "ok".into(),
            },
            IpcResponse::Status { daemon: status() },
            IpcResponse::Error {
                message: "bad".into(),
            },
        ] {
            let s = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<IpcResponse>(&s).unwrap(), r);
        }
    }

    #[test]
    fn unix_socket_roundtrip() {
        let path = std::env::temp_dir().join(format!("nxctl-ipc-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let expected = IpcResponse::Status { daemon: status() };
        let reply = expected.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: IpcRequest = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(req, IpcRequest::Status);
            let mut out = stream;
            writeln!(out, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
        });
        let got = send_request(&path, &IpcRequest::Status).unwrap();
        handle.join().unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(got, expected);
    }
}
