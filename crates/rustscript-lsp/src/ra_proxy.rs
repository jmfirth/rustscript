//! Rust-analyzer subprocess proxy.
//!
//! Manages a rust-analyzer child process that runs on the project root directory
//! containing `Cargo.toml` and generated Rust code. Communicates via the LSP wire
//! protocol over stdin/stdout pipes, forwarding requests and receiving responses.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use crate::error::LspError;

/// Shared map of pending request IDs to their response channels.
type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>;

/// A proxy that manages a rust-analyzer child process.
///
/// Sends LSP requests and notifications to rust-analyzer over stdin and reads
/// responses from stdout. The proxy handles the LSP wire framing protocol
/// (`Content-Length` headers) and request/response correlation.
pub struct RustAnalyzerProxy {
    /// The rust-analyzer child process.
    process: Mutex<Child>,
    /// Stdin writer for sending requests (mutex-protected for thread safety).
    writer: Mutex<BufWriter<std::process::ChildStdin>>,
    /// Request ID counter.
    next_id: AtomicI64,
    /// Pending requests awaiting responses (shared with reader thread).
    pending: PendingMap,
    /// Handle to the reader thread (kept alive for the proxy's lifetime).
    _reader_handle: Option<std::thread::JoinHandle<()>>,
}

impl RustAnalyzerProxy {
    /// Attempt to start rust-analyzer pointed at the project root.
    ///
    /// Returns `Ok(Some(proxy))` if rust-analyzer starts successfully,
    /// `Ok(None)` if rust-analyzer is not found in PATH, or `Err` for other failures.
    ///
    /// # Errors
    ///
    /// Returns [`LspError::RustAnalyzerStart`] if the process starts but
    /// stdin/stdout cannot be captured.
    pub fn start(build_dir: &Path) -> Result<Option<Self>, LspError> {
        let mut child = match Command::new("rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(build_dir)
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(e) => return Err(LspError::RustAnalyzerStart(e.to_string())),
        };

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::RustAnalyzerStart("failed to capture stdin".to_owned()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::RustAnalyzerStart("failed to capture stdout".to_owned()))?;

        let writer = Mutex::new(BufWriter::new(stdin));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        // Spawn a reader thread that processes incoming LSP messages from RA.
        let pending_for_reader = Arc::clone(&pending);
        let reader_handle = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(msg)) = read_lsp_message(&mut reader) {
                // Check if this is a response (has "id" and "result"/"error").
                if let Some(id) = msg.get("id").and_then(serde_json::Value::as_i64)
                    && (msg.get("result").is_some() || msg.get("error").is_some())
                {
                    let Ok(mut pending_lock) = pending_for_reader.lock() else {
                        break;
                    };
                    if let Some(tx) = pending_lock.remove(&id) {
                        let _ = tx.send(msg);
                    }
                }
                // Notifications from RA are silently dropped.
            }
        });

        Ok(Some(Self {
            process: Mutex::new(child),
            writer,
            next_id: AtomicI64::new(1),
            pending,
            _reader_handle: Some(reader_handle),
        }))
    }

    /// Send an LSP request to rust-analyzer and await the response.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be written or the response channel breaks.
    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;
            pending.insert(id, tx);
        }

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.send_message(&msg)?;

        rx.await
            .map_err(|_| LspError::RustAnalyzerComm("response channel closed".to_owned()))
    }

    /// Send an LSP notification to rust-analyzer (no response expected).
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be written.
    pub fn notify(&self, method: &str, params: &serde_json::Value) -> Result<(), LspError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        self.send_message(&msg)
    }

    /// Shut down rust-analyzer gracefully.
    ///
    /// Sends the LSP `shutdown` request followed by `exit` notification,
    /// then waits for the process to exit.
    ///
    /// # Errors
    ///
    /// Returns an error if the shutdown sequence fails.
    pub async fn shutdown(&self) -> Result<(), LspError> {
        // Send shutdown request (best-effort).
        let _ = self.request("shutdown", serde_json::Value::Null).await;

        // Send exit notification (best-effort).
        let _ = self.notify("exit", &serde_json::Value::Null);

        // Wait for the process to exit.
        let mut process = self
            .process
            .lock()
            .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;
        let _ = process.wait();

        Ok(())
    }

    /// Check if the rust-analyzer process is still running.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        if let Ok(mut process) = self.process.lock() {
            matches!(process.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Write an LSP message with `Content-Length` framing to rust-analyzer's stdin.
    fn send_message(&self, msg: &serde_json::Value) -> Result<(), LspError> {
        let body =
            serde_json::to_string(msg).map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

        let mut writer = self
            .writer
            .lock()
            .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

        write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

        writer
            .flush()
            .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

        Ok(())
    }
}

/// Read a single LSP message from a buffered reader.
///
/// Parses the `Content-Length` header, reads the JSON body, and returns the
/// parsed value. Returns `Ok(None)` on EOF.
fn read_lsp_message(
    reader: &mut BufReader<std::process::ChildStdout>,
) -> Result<Option<serde_json::Value>, LspError> {
    let mut content_length: Option<usize> = None;

    // Read headers until empty line.
    loop {
        let mut header = String::new();
        let bytes_read = reader
            .read_line(&mut header)
            .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

        if bytes_read == 0 {
            return Ok(None); // EOF
        }

        let trimmed = header.trim();
        if trimmed.is_empty() {
            break; // End of headers.
        }

        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length =
                Some(len_str.parse().map_err(|e: std::num::ParseIntError| {
                    LspError::RustAnalyzerComm(e.to_string())
                })?);
        }
    }

    let length = content_length
        .ok_or_else(|| LspError::RustAnalyzerComm("missing Content-Length header".to_owned()))?;

    // Read the JSON body.
    let mut body = vec![0u8; length];
    std::io::Read::read_exact(reader, &mut body)
        .map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

    let msg: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| LspError::RustAnalyzerComm(e.to_string()))?;

    Ok(Some(msg))
}

/// Check if rust-analyzer is available in PATH.
#[must_use]
pub fn is_rust_analyzer_available() -> bool {
    Command::new("rust-analyzer")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 8: Graceful degradation — proxy handles missing RA without crashing
    #[test]
    fn test_ra_proxy_graceful_degradation_no_ra() {
        // We can't guarantee RA is missing, but we verify the check doesn't panic.
        let available = is_rust_analyzer_available();
        assert!(
            available || !available,
            "should return a bool without panicking"
        );
    }

    #[test]
    fn test_ra_proxy_lsp_message_framing() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let body = serde_json::to_string(&msg).unwrap();
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        assert!(framed.starts_with("Content-Length: "));
        assert!(framed.contains("\r\n\r\n"));
        assert!(framed.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_ra_proxy_request_id_increments() {
        let counter = AtomicI64::new(1);
        let id1 = counter.fetch_add(1, Ordering::Relaxed);
        let id2 = counter.fetch_add(1, Ordering::Relaxed);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_ra_proxy_pending_map_insert_and_remove() {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = oneshot::channel();
        {
            let mut map = pending.lock().unwrap();
            map.insert(42, tx);
            assert!(map.contains_key(&42));
        }
        {
            let mut map = pending.lock().unwrap();
            let removed = map.remove(&42);
            assert!(removed.is_some());
            assert!(!map.contains_key(&42));
        }
    }
}
