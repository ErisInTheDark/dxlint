//! JSON-RPC over stdio with LSP's `Content-Length` framing.

use std::io::{self, BufRead, Write};
use std::sync::mpsc;

use serde_json::Value;

/// Reads one message; `Ok(None)` on a clean end of input.
pub fn read(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let mut body = vec![0; length.unwrap_or(0)];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_string(msg)?;
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// Cloneable handle that serialises every outgoing message through one
/// writer thread, so background scans can publish while requests run.
#[derive(Clone)]
pub struct Outbox(mpsc::Sender<Value>);

impl Outbox {
    pub fn spawn<W: Write + Send + 'static>(mut out: W) -> (Outbox, std::thread::JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<Value>();
        let handle = std::thread::spawn(move || {
            for msg in rx {
                if write(&mut out, &msg).is_err() {
                    break;
                }
            }
        });
        (Outbox(tx), handle)
    }

    pub fn send(&self, msg: Value) {
        let _ = self.0.send(msg);
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    pub fn respond(&self, id: Value, result: Value) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
    }

    pub fn request(&self, id: &str, method: &str, params: Value) {
        self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips() {
        let mut buf = Vec::new();
        write(&mut buf, &serde_json::json!({"a": "é"})).unwrap();
        write(&mut buf, &serde_json::json!({"b": 2})).unwrap();
        let mut input = io::BufReader::new(&buf[..]);
        assert_eq!(read(&mut input).unwrap().unwrap()["a"], "é");
        assert_eq!(read(&mut input).unwrap().unwrap()["b"], 2);
        assert!(read(&mut input).unwrap().is_none());
    }
}
