use std::collections::VecDeque;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct McpCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
    pub max_frame_bytes: usize,
}

#[derive(Debug)]
pub enum McpError {
    Spawn(std::io::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Timeout(Duration),
    Closed,
    InvalidFrame(String),
    Rpc(Value),
}

impl Display for McpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "failed to start MCP server: {error}"),
            Self::Io(error) => write!(formatter, "MCP transport error: {error}"),
            Self::Json(error) => write!(formatter, "invalid MCP JSON: {error}"),
            Self::Timeout(timeout) => write!(formatter, "MCP response timed out after {timeout:?}"),
            Self::Closed => formatter.write_str("MCP server closed its output"),
            Self::InvalidFrame(message) => write!(formatter, "invalid MCP frame: {message}"),
            Self::Rpc(error) => write!(formatter, "MCP server returned an error: {error}"),
        }
    }
}

impl std::error::Error for McpError {}

impl From<std::io::Error> for McpError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for McpError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub struct McpChild {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Result<Vec<u8>, String>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    timeout: Duration,
    next_id: u64,
}

impl McpChild {
    pub fn spawn(command: &McpCommand) -> Result<Self, McpError> {
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &command.cwd {
            process.current_dir(cwd);
        }
        let mut child = process.spawn().map_err(McpError::Spawn)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::InvalidFrame("child stdin was not available".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::InvalidFrame("child stdout was not available".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| McpError::InvalidFrame("child stderr was not available".to_owned()))?;

        let (sender, frames) = mpsc::sync_channel(16);
        let max_frame_bytes = command.max_frame_bytes;
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let message = read_bounded_line(&mut reader, max_frame_bytes)
                    .map_err(|error| error.to_string());
                match message {
                    Ok(Some(frame)) => {
                        if sender.send(Ok(frame)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });

        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(20)));
        let stderr_target = Arc::clone(&stderr_tail);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut tail) = stderr_target.lock() {
                    if tail.len() == 20 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            frames,
            stderr_tail,
            timeout: command.timeout,
            next_id: 1,
        })
    }

    pub fn initialize(&mut self) -> Result<Value, McpError> {
        let result = self.request(
            "initialize",
            &json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "cortex-loom", "version": env!("CARGO_PKG_VERSION")}
            }),
        )?;
        self.notify("notifications/initialized", &json!({}))?;
        Ok(result)
    }

    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<Value, McpError> {
        self.request("tools/call", &json!({"name": name, "arguments": arguments}))
    }

    pub fn request(&mut self, method: &str, params: &Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;

        loop {
            let frame = self
                .frames
                .recv_timeout(self.timeout)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => McpError::Timeout(self.timeout),
                    mpsc::RecvTimeoutError::Disconnected => McpError::Closed,
                })?
                .map_err(McpError::InvalidFrame)?;
            let response: Value = serde_json::from_slice(&frame)?;
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(McpError::Rpc(error.clone()));
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| McpError::InvalidFrame("response has no result".to_owned()));
        }
    }

    pub fn notify(&mut self, method: &str, params: &Value) -> Result<(), McpError> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    #[must_use]
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .map_or_else(|_| Vec::new(), |tail| tail.iter().cloned().collect())
    }

    fn write_message(&mut self, message: &Value) -> Result<(), McpError> {
        serde_json::to_writer(&mut self.stdin, message)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_bounded_line<R: Read>(
    reader: &mut BufReader<R>,
    max: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::with_capacity(max.min(8192));
    let mut overflow = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if frame.is_empty() && !overflow {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "incomplete MCP frame at EOF",
            ));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(buffer.len(), |index| index + 1);
        let content_len = newline.unwrap_or(buffer.len());
        if !overflow {
            if frame.len().saturating_add(content_len) > max {
                overflow = true;
            } else {
                frame.extend_from_slice(&buffer[..content_len]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            if overflow {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "MCP frame exceeds configured byte limit",
                ));
            }
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            return Ok(Some(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_rejects_oversize_and_incomplete_frames() {
        let mut oversized = BufReader::new(&b"123456\n"[..]);
        assert!(read_bounded_line(&mut oversized, 4).is_err());

        let mut incomplete = BufReader::new(&b"{}"[..]);
        assert_eq!(
            read_bounded_line(&mut incomplete, 8)
                .expect_err("partial EOF must fail")
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn bounded_reader_accepts_crlf() {
        let mut reader = BufReader::new(&b"{}\r\n"[..]);
        assert_eq!(
            read_bounded_line(&mut reader, 8).expect("frame"),
            Some(b"{}".to_vec())
        );
    }
}
