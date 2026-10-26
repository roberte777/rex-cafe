//! One control connection to the Rex server: a WebSocket over its unix socket.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

const API_REVISION: u64 = 7;
const SUBPROTOCOL: &str = "rex.control.v1";

/// An error the server returned for a request.
#[derive(Debug, Clone)]
pub struct RexError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for RexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RexError {}

type Reply = Result<(u64, Value), RexError>;

struct Inner {
    out: mpsc::UnboundedSender<Message>,
    pending: Mutex<HashMap<String, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
}

/// A handle for making requests. Cloning shares the connection.
#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

/// An event from the server with its place in the stream.
pub type Event = (u64, Value);

/// Finds the server socket from `REX_SERVER`, or Rex's default location.
pub fn socket_path() -> PathBuf {
    if let Ok(url) = std::env::var("REX_SERVER")
        && let Some(path) = url.strip_prefix("unix://")
    {
        return PathBuf::from(percent_decode(path));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/rex/server.sock")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Connection {
    /// Connects, says hello, and starts reading. Events arrive on the returned channel,
    /// which closes when the connection does.
    pub async fn open(path: &Path) -> Result<(Connection, mpsc::UnboundedReceiver<Event>)> {
        let stream = UnixStream::connect(path)
            .await
            .with_context(|| format!("connecting to {}", path.display()))?;
        let mut request = "ws://localhost/api/control".into_client_request()?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_static(SUBPROTOCOL),
        );
        let (ws, _) = tokio_tungstenite::client_async(request, stream)
            .await
            .context("opening the control WebSocket")?;
        let (mut sink, mut source) = ws.split();

        let hello = json!({
            "type": "hello",
            "api_revision": API_REVISION,
            "client": {"kind": "other", "name": "rex-cafe", "version": env!("CARGO_PKG_VERSION")},
        });
        sink.send(Message::Text(hello.to_string().into())).await?;
        let welcome = tokio::time::timeout(Duration::from_secs(5), source.next())
            .await
            .context("waiting for the server's welcome")?
            .ok_or_else(|| anyhow!("the server closed the connection before welcoming us"))??;
        let welcome: Value = serde_json::from_str(welcome.to_text()?)?;
        if welcome["type"] != "welcome" || welcome["protocol"] != SUBPROTOCOL {
            bail!("unexpected first message from the server: {welcome}");
        }

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            out: out_tx,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let reader = Arc::clone(&inner);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = source.next().await {
                let Message::Text(text) = msg else {
                    if matches!(msg, Message::Close(_)) {
                        break;
                    }
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match v["type"].as_str() {
                    Some("response") => {
                        let id = v["id"].as_str().unwrap_or_default();
                        let Some(tx) = reader.pending.lock().unwrap().remove(id) else {
                            continue;
                        };
                        let seq = v["seq"].as_u64().unwrap_or(0);
                        let reply = match v.get("error").filter(|e| !e.is_null()) {
                            Some(e) => Err(RexError {
                                code: e["code"].as_str().unwrap_or("internal").to_string(),
                                message: e["message"].as_str().unwrap_or_default().to_string(),
                            }),
                            None => Ok((seq, v.get("payload").cloned().unwrap_or(Value::Null))),
                        };
                        let _ = tx.send(reply);
                    }
                    Some("event") => {
                        let seq = v["seq"].as_u64().unwrap_or(0);
                        if event_tx.send((seq, v["payload"].clone())).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            // Fail every request still waiting; dropping the senders does that.
            reader.pending.lock().unwrap().clear();
        });

        Ok((Connection { inner }, event_rx))
    }

    /// Calls a method and returns the response's `seq` with its payload.
    pub async fn call(&self, method: &str, payload: Value) -> Result<(u64, Value)> {
        let id = self
            .inner
            .next_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string();
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(id.clone(), tx);
        let req = json!({"type": "request", "id": id, "method": method, "payload": payload});
        self.inner
            .out
            .send(Message::Text(Utf8Bytes::from(req.to_string())))
            .map_err(|_| anyhow!("not connected to Rex"))?;
        let reply = tokio::time::timeout(Duration::from_secs(15), rx)
            .await
            .map_err(|_| anyhow!("{method} timed out"))?
            .map_err(|_| anyhow!("lost the connection to Rex"))?;
        Ok(reply?)
    }

    /// Calls a block's own method, such as `com.superlogical.terminal.title`.
    pub async fn block_call(
        &self,
        session: &str,
        block: &str,
        method: &str,
    ) -> Result<(u64, Value)> {
        self.call(
            &format!("com.superlogical.terminal.{method}"),
            json!({"session_id": session, "block_id": block, "args": {}}),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_socket_paths() {
        assert_eq!(
            percent_decode("/a/Application%20Support/rex.sock"),
            "/a/Application Support/rex.sock"
        );
        assert_eq!(percent_decode("/plain"), "/plain");
        assert_eq!(percent_decode("/bad%2"), "/bad%2");
    }
}
