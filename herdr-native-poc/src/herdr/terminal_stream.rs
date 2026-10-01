//! Observe and control sessions over Herdr NDJSON streams.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::herdr::HerdrError;
use crate::terminal::{FrameBytes, TerminalCloseReason, TerminalEvent};

const CHANNEL_BOUND: usize = 8;
const STDERR_CAP: usize = 4 * 1024;
const CONTROL_REJECT_PREFIX: &str = "herdr: terminal session control input ignored:";

/// Parses observe/control stdout NDJSON, tracking `seq` for resync detection.
struct FrameParser {
    last_seq: Option<u64>,
}

impl FrameParser {
    fn new() -> Self {
        Self { last_seq: None }
    }

    fn ingest(&mut self, line: &str) -> Result<TerminalEvent, HerdrError> {
        let line = line.trim();
        if line.is_empty() {
            return Err(HerdrError::InvalidNdjson);
        }
        let value: Value = serde_json::from_str(line).map_err(|_| HerdrError::InvalidNdjson)?;
        let ty = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or(HerdrError::InvalidNdjson)?;
        match ty {
            "terminal.frame" => self.ingest_frame(&value),
            "terminal.closed" => {
                let reason = value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                Ok(TerminalEvent::Closed(TerminalCloseReason { reason }))
            }
            _ => Err(HerdrError::InvalidNdjson),
        }
    }

    fn ingest_frame(&mut self, value: &Value) -> Result<TerminalEvent, HerdrError> {
        let seq = value
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or(HerdrError::InvalidNdjson)?;
        let full = value
            .get("full")
            .and_then(Value::as_bool)
            .ok_or(HerdrError::InvalidNdjson)?;
        let encoding = value
            .get("encoding")
            .and_then(Value::as_str)
            .ok_or(HerdrError::InvalidNdjson)?;
        if encoding != "ansi" {
            return Err(HerdrError::UnexpectedFrameEncoding);
        }
        let cols = value
            .get("width")
            .and_then(Value::as_u64)
            .ok_or(HerdrError::InvalidNdjson)? as u16;
        let rows = value
            .get("height")
            .and_then(Value::as_u64)
            .ok_or(HerdrError::InvalidNdjson)? as u16;

        // Validate payload before seq so bad base64 / missing bytes surface first.
        let b64 = match value.get("bytes") {
            None | Some(Value::Null) => return Err(HerdrError::MissingFrameBytes),
            Some(v) => v.as_str().ok_or(HerdrError::MissingFrameBytes)?,
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| HerdrError::InvalidFrameBase64)?;

        if let Some(last) = self.last_seq {
            if seq <= last {
                return Err(HerdrError::SequenceResyncRequired);
            }
        } else if seq != 1 || !full {
            // A new process starts at seq 1 with full true.
            return Err(HerdrError::SequenceResyncRequired);
        }

        self.last_seq = Some(seq);
        Ok(TerminalEvent::Data(FrameBytes {
            bytes,
            full,
            seq,
            cols,
            rows,
        }))
    }
}

type StreamMsg = Result<TerminalEvent, HerdrError>;

struct ChildSession {
    child: Child,
    rx: Receiver<StreamMsg>,
    _stdout_thread: Option<JoinHandle<()>>,
    _stderr_thread: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<String>>,
    finished: Arc<AtomicBool>,
}

impl ChildSession {
    fn spawn_reader(
        mut child: Child,
        control_mode: bool,
    ) -> Result<(Self, Option<std::process::ChildStdin>), HerdrError> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HerdrError::Io("missing stdout pipe".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| HerdrError::Io("missing stderr pipe".into()))?;
        let stdin = child.stdin.take();

        let (tx, rx) = sync_channel::<StreamMsg>(CHANNEL_BOUND);
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let finished = Arc::new(AtomicBool::new(false));

        let tx_out = tx.clone();
        let finished_out = Arc::clone(&finished);
        let stdout_thread = thread::spawn(move || {
            let mut parser = FrameParser::new();
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let msg = parser.ingest(&line);
                        // Never log frame payloads.
                        if tx_out.send(msg).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx_out.send(Err(HerdrError::Io(e.to_string())));
                        break;
                    }
                }
            }
            finished_out.store(true, Ordering::SeqCst);
        });

        let stderr_buf_err = Arc::clone(&stderr_buf);
        let tx_err = tx;
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                {
                    let mut buf = stderr_buf_err.lock().unwrap_or_else(|e| e.into_inner());
                    if buf.len() < STDERR_CAP {
                        let room = STDERR_CAP - buf.len();
                        let take = line.chars().take(room).collect::<String>();
                        if !buf.is_empty() {
                            buf.push('\n');
                        }
                        buf.push_str(&take);
                    }
                }
                if control_mode && line.starts_with(CONTROL_REJECT_PREFIX) {
                    let detail = line[CONTROL_REJECT_PREFIX.len()..].trim().to_string();
                    let _ = tx_err.send(Err(HerdrError::ControlInputRejected(detail)));
                }
            }
        });

        Ok((
            Self {
                child,
                rx,
                _stdout_thread: Some(stdout_thread),
                _stderr_thread: Some(stderr_thread),
                stderr: stderr_buf,
                finished,
            },
            stdin,
        ))
    }

    fn recv(&mut self) -> Result<TerminalEvent, HerdrError> {
        match self.rx.recv() {
            Ok(msg) => msg,
            Err(_) => self.map_disconnect(),
        }
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<TerminalEvent, HerdrError> {
        match self.rx.recv_timeout(timeout) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => Err(HerdrError::Timeout),
            Err(RecvTimeoutError::Disconnected) => self.map_disconnect(),
        }
    }

    fn map_disconnect(&mut self) -> Result<TerminalEvent, HerdrError> {
        let stderr = self.stderr.lock().map(|g| g.clone()).unwrap_or_default();
        match self.child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let code = status.code().unwrap_or(-1);
                    return Err(HerdrError::ProcessExit {
                        status: code,
                        stderr,
                    });
                }
                if !stderr.is_empty() {
                    return Err(HerdrError::Stderr(stderr));
                }
                Err(HerdrError::StreamEof)
            }
            Ok(None) => {
                if self.finished.load(Ordering::SeqCst) {
                    let _ = self.child.wait();
                    Err(HerdrError::StreamEof)
                } else if !stderr.is_empty() {
                    Err(HerdrError::Stderr(stderr))
                } else {
                    Err(HerdrError::StreamEof)
                }
            }
            Err(e) => Err(HerdrError::Io(e.to_string())),
        }
    }
}

impl ChildSession {
    /// Wait until the child exits or `timeout` elapses. Does not kill the child.
    fn wait_exit(&mut self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if start.elapsed() >= timeout => return false,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => return false,
            }
        }
    }
}

impl Drop for ChildSession {
    fn drop(&mut self) {
        // Last resort after an orderly `terminal.release`. Kill does not
        // guarantee Herdr observed that record; Herdr detaches on stdin EOF.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ObserveSession {
    inner: ChildSession,
}

impl ObserveSession {
    /// Argv for `herdr terminal session observe` (never includes `--takeover`).
    pub fn argv(pane_id: &str, cols: u16, rows: u16) -> Vec<String> {
        vec![
            "herdr".into(),
            "terminal".into(),
            "session".into(),
            "observe".into(),
            pane_id.into(),
            "--cols".into(),
            cols.to_string(),
            "--rows".into(),
            rows.to_string(),
        ]
    }

    pub fn spawn(pane_id: &str, cols: u16, rows: u16) -> Result<Self, HerdrError> {
        let argv = Self::argv(pane_id, cols, rows);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| HerdrError::Command(e.to_string()))?;
        let (inner, _stdin) = ChildSession::spawn_reader(child, false)?;
        Ok(Self { inner })
    }

    pub fn recv(&mut self) -> Result<TerminalEvent, HerdrError> {
        self.inner.recv()
    }

    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<TerminalEvent, HerdrError> {
        self.inner.recv_timeout(timeout)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollSource {
    Wheel,
    PageKey,
}

impl ScrollSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::PageKey => "page_key",
        }
    }
}

/// NDJSON body for `terminal.scroll`.
///
/// `column` and `row` are the cell under the pointer. Herdr uses them when the
/// pane's TUI is tracking the mouse; without them the wheel is reported at 0,0.
pub fn scroll_command(
    direction: ScrollDirection,
    lines: u16,
    source: ScrollSource,
    column: Option<u16>,
    row: Option<u16>,
) -> Value {
    let mut line = json!({
        "type": "terminal.scroll",
        "direction": direction.as_str(),
        "lines": lines,
        "source": source.as_str(),
    });
    if let Some(column) = column {
        line["column"] = json!(column);
    }
    if let Some(row) = row {
        line["row"] = json!(row);
    }
    line
}

pub struct ControlSession {
    inner: ChildSession,
    stdin: Mutex<std::process::ChildStdin>,
}

impl ControlSession {
    /// Argv for `herdr terminal session control`; adds `--takeover` only when requested.
    pub fn argv(pane_id: &str, cols: u16, rows: u16, takeover: bool) -> Vec<String> {
        let mut argv = vec![
            "herdr".into(),
            "terminal".into(),
            "session".into(),
            "control".into(),
            pane_id.into(),
            "--cols".into(),
            cols.to_string(),
            "--rows".into(),
            rows.to_string(),
        ];
        if takeover {
            argv.push("--takeover".into());
        }
        argv
    }

    pub fn spawn(pane_id: &str, cols: u16, rows: u16, takeover: bool) -> Result<Self, HerdrError> {
        let argv = Self::argv(pane_id, cols, rows, takeover);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| HerdrError::Command(e.to_string()))?;
        let (inner, stdin) = ChildSession::spawn_reader(child, true)?;
        let stdin = stdin.ok_or_else(|| HerdrError::Io("missing stdin pipe".into()))?;
        Ok(Self {
            inner,
            stdin: Mutex::new(stdin),
        })
    }

    pub fn recv(&mut self) -> Result<TerminalEvent, HerdrError> {
        self.inner.recv()
    }

    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<TerminalEvent, HerdrError> {
        self.inner.recv_timeout(timeout)
    }

    pub fn input_text(&mut self, text: &str) -> Result<(), HerdrError> {
        let line = json!({"type": "terminal.input", "text": text});
        self.write_json_line(&line)
    }

    pub fn input_bytes(&mut self, bytes: &[u8]) -> Result<(), HerdrError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let line = json!({"type": "terminal.input", "bytes": b64});
        self.write_json_line(&line)
    }

    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<(), HerdrError> {
        if cols == 0 || rows == 0 {
            return Err(HerdrError::InvalidResize);
        }
        let mut line = json!({
            "type": "terminal.resize",
            "cols": cols,
            "rows": rows,
        });
        if cell_width_px != 0 {
            line["cell_width_px"] = json!(cell_width_px);
        }
        if cell_height_px != 0 {
            line["cell_height_px"] = json!(cell_height_px);
        }
        self.write_json_line(&line)
    }

    pub fn scroll(
        &mut self,
        direction: ScrollDirection,
        lines: u16,
        source: ScrollSource,
        column: Option<u16>,
        row: Option<u16>,
    ) -> Result<(), HerdrError> {
        if lines == 0 {
            return Err(HerdrError::Io(
                "terminal.scroll lines must be greater than 0".into(),
            ));
        }
        self.write_json_line(&scroll_command(direction, lines, source, column, row))
    }

    pub fn release(&mut self) -> Result<(), HerdrError> {
        let line = json!({"type": "terminal.release"});
        self.write_json_line(&line)
    }

    /// Flush `terminal.release`, then wait for the child to exit.
    ///
    /// `Ok(true)` means the process exited inside `timeout`. `Ok(false)` means
    /// the release line was flushed and the process was still running. Drop
    /// then kills it. Forced termination of this process before the flush
    /// cannot guarantee detach.
    pub fn release_with_timeout(&mut self, timeout: Duration) -> Result<bool, HerdrError> {
        self.release()?;
        Ok(self.inner.wait_exit(timeout))
    }

    fn write_json_line(&mut self, value: &Value) -> Result<(), HerdrError> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|e| HerdrError::Io(e.to_string()))?;
        let mut raw = serde_json::to_string(value).map_err(|e| HerdrError::Io(e.to_string()))?;
        raw.push('\n');
        stdin
            .write_all(raw.as_bytes())
            .map_err(|e| HerdrError::Io(e.to_string()))?;
        stdin.flush().map_err(|e| HerdrError::Io(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/contracts/fixtures")
    }

    fn read_fixture(name: &str) -> String {
        fs::read_to_string(fixtures_dir().join(name)).unwrap()
    }

    #[test]
    fn initial_stream_decodes_red_ansi_full_frame() {
        let text = read_fixture("initial-stream.ndjson");
        let mut parser = FrameParser::new();
        let line = text.lines().next().unwrap();
        let ev = parser.ingest(line).unwrap();
        match ev {
            TerminalEvent::Data(fb) => {
                assert_eq!(fb.bytes, b"\x1b[31mred\x1b[0m");
                assert!(fb.full);
                assert_eq!(fb.seq, 1);
                assert_eq!(fb.cols, 120);
                assert_eq!(fb.rows, 40);
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn close_fixture_emits_closed() {
        let text = read_fixture("close.ndjson");
        let mut parser = FrameParser::new();
        let ev = parser.ingest(text.lines().next().unwrap()).unwrap();
        match ev {
            TerminalEvent::Closed(c) => {
                assert!(c.reason.contains("not found"));
            }
            other => panic!("expected Closed, got {other:?}"),
        }
    }

    #[test]
    fn malformed_record_errors_without_payload_in_display() {
        let text = read_fixture("malformed-record.ndjson");
        let mut parser = FrameParser::new();
        let mut lines = text.lines();
        let err = parser.ingest(lines.next().unwrap()).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("G1szMW1yZWQbWzBt"), "{msg}");
        // Second line is valid after a fresh parser (malformed does not advance state
        // when ingest fails before seq update — start fresh to show deterministic OK).
        let mut parser2 = FrameParser::new();
        let ok = parser2.ingest(lines.next().unwrap()).unwrap();
        assert!(matches!(ok, TerminalEvent::Data(_)));
    }

    #[test]
    fn invalid_payload_missing_bytes_and_bad_base64() {
        let text = read_fixture("invalid-payload.ndjson");
        let mut lines = text.lines();

        let mut parser = FrameParser::new();
        let err1 = parser.ingest(lines.next().unwrap()).unwrap_err();
        assert!(matches!(err1, HerdrError::MissingFrameBytes));

        let err2 = parser.ingest(lines.next().unwrap()).unwrap_err();
        assert!(matches!(err2, HerdrError::InvalidFrameBase64), "{err2:?}");
        assert!(!err2.to_string().contains("!!!not-base64!!!"));

        // Empty `bytes` is valid standard base64 of empty after a real seq-1 full frame.
        let mut parser3 = FrameParser::new();
        let prime = read_fixture("initial-stream.ndjson");
        parser3.ingest(prime.lines().next().unwrap()).unwrap();
        let ev = parser3.ingest(lines.next().unwrap()).unwrap();
        match ev {
            TerminalEvent::Data(fb) => {
                assert!(fb.bytes.is_empty());
                assert_eq!(fb.seq, 3);
            }
            other => panic!("expected empty Data, got {other:?}"),
        }
    }

    #[test]
    fn seq_backwards_demands_resync_without_emitting_data() {
        let mut parser = FrameParser::new();
        let first = read_fixture("initial-stream.ndjson");
        parser.ingest(first.lines().next().unwrap()).unwrap();
        let err = parser
            .ingest(r#"{"type":"terminal.frame","encoding":"ansi","bytes":"G1szMW1yZWQbWzBt","seq":1,"full":true,"width":120,"height":40}"#)
            .unwrap_err();
        assert!(matches!(err, HerdrError::SequenceResyncRequired));
        assert!(err.to_string().contains("resync"));
    }

    #[test]
    fn observe_argv_has_dims_never_takeover() {
        let argv = ObserveSession::argv("w9:p2", 120, 40);
        assert_eq!(
            argv,
            vec![
                "herdr", "terminal", "session", "observe", "w9:p2", "--cols", "120", "--rows",
                "40",
            ]
        );
        assert!(!argv.iter().any(|a| a == "--takeover"));
    }

    #[test]
    fn control_argv_takeover_only_when_requested() {
        let no = ControlSession::argv("w9:p2", 80, 24, false);
        assert!(!no.iter().any(|a| a == "--takeover"));
        assert_eq!(
            no,
            vec![
                "herdr", "terminal", "session", "control", "w9:p2", "--cols", "80", "--rows", "24",
            ]
        );
        let yes = ControlSession::argv("w9:p2", 80, 24, true);
        assert!(yes.ends_with(&["--takeover".to_string()]));
    }

    #[test]
    fn scroll_command_names_direction_lines_and_source() {
        let line = scroll_command(
            ScrollDirection::Up,
            3,
            ScrollSource::Wheel,
            Some(12),
            Some(4),
        );
        assert_eq!(line["type"], "terminal.scroll");
        assert_eq!(line["direction"], "up");
        assert_eq!(line["lines"], 3);
        assert_eq!(line["source"], "wheel");
        assert_eq!(line["column"], 12);
        assert_eq!(line["row"], 4);
        let page = scroll_command(ScrollDirection::Down, 20, ScrollSource::PageKey, None, None);
        assert_eq!(page["direction"], "down");
        assert_eq!(page["source"], "page_key");
        assert!(page.get("column").is_none());
        assert!(page.get("row").is_none());
    }

    #[test]
    fn control_resize_json_omits_zero_cell_px() {
        // Build the same JSON the write path would — via public reshape of fields.
        let mut line = json!({
            "type": "terminal.resize",
            "cols": 100u16,
            "rows": 30u16,
        });
        let cell_width_px = 0u32;
        let cell_height_px = 0u32;
        if cell_width_px != 0 {
            line["cell_width_px"] = json!(cell_width_px);
        }
        if cell_height_px != 0 {
            line["cell_height_px"] = json!(cell_height_px);
        }
        let s = serde_json::to_string(&line).unwrap();
        assert!(!s.contains("cell_width_px"));
        assert!(!s.contains("cell_height_px"));
    }

    #[test]
    fn incremental_fixture_is_data_not_drop() {
        let text = read_fixture("incremental-update.ndjson");
        let mut parser = FrameParser::new();
        let mut lines = text.lines();
        let first = parser.ingest(lines.next().unwrap()).unwrap();
        assert!(matches!(first, TerminalEvent::Data(ref f) if f.full && f.seq == 1));
        let second = parser.ingest(lines.next().unwrap()).unwrap();
        match second {
            TerminalEvent::Data(f) => {
                assert!(!f.full);
                assert_eq!(f.seq, 2);
            }
            other => panic!("{other:?}"),
        }
    }
}
