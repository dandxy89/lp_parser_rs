//! End-to-end: drive the server in-process through the tower service.
//! initialize → didOpen → diagnostics → didChange → updated diagnostics →
//! rename → formatting → shutdown.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use lp_lsp::Backend;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::{Request, Response};

const URI: &str = "file:///tmp/e2e.lp";
const TEXT: &str = "Minimize\n obj:  3 x+2 y\nSubject To\n c1: x + y >= 2\n c1: x - y <= 8\nBounds\n x <= 10\nEnd\n";

struct Harness {
    service: LspService<Backend>,
    notifications: mpsc::UnboundedReceiver<(String, Value)>,
    next_id: i64,
}

impl Harness {
    fn start() -> Self {
        let (service, socket) = LspService::new(Backend::new);
        let (tx, notifications) = mpsc::unbounded_channel();
        let (mut requests, mut responses) = socket.split();
        // Play the client: forward notifications, answer server requests with `null`.
        tokio::spawn(async move {
            while let Some(request) = requests.next().await {
                let (method, id, params) = request.into_parts();
                match id {
                    Some(id) => {
                        if responses.send(Response::from_ok(id, Value::Null)).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        if tx.send((method.into_owned(), params.unwrap_or(Value::Null))).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self { service, notifications, next_id: 1 }
    }

    async fn request(&mut self, method: &'static str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request::build(method).id(id).params(params).finish();
        let response = self.service.ready().await.expect("service ready").call(request).await.expect("service call").expect("a response");
        let (_, body) = response.into_parts();
        body.unwrap_or_else(|e| panic!("{method} failed: {e:?}"))
    }

    async fn notify(&mut self, method: &'static str, params: Value) {
        let request = Request::build(method).params(params).finish();
        let response = self.service.ready().await.expect("service ready").call(request).await.expect("service call");
        assert!(response.is_none(), "notifications have no response");
    }

    /// Next `publishDiagnostics` for [`URI`], skipping other notifications.
    async fn diagnostics(&mut self) -> Vec<Value> {
        let deadline = Duration::from_secs(10);
        loop {
            let (method, params) =
                tokio::time::timeout(deadline, self.notifications.recv()).await.expect("diagnostics in time").expect("channel open");
            if method == "textDocument/publishDiagnostics" && params["uri"] == URI {
                return params["diagnostics"].as_array().cloned().unwrap_or_default();
            }
        }
    }
}

fn codes(diagnostics: &[Value]) -> Vec<String> {
    diagnostics.iter().filter_map(|d| d["code"].as_str().map(str::to_owned)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn full_session() {
    let mut h = Harness::start();

    let init = h
        .request("initialize", json!({ "capabilities": { "textDocument": { "publishDiagnostics": {} }, "general": { "positionEncodings": ["utf-8", "utf-16"] } } }))
        .await;
    assert_eq!(init["capabilities"]["positionEncoding"], "utf-8");
    assert_eq!(init["capabilities"]["textDocumentSync"]["change"], 2, "incremental sync");
    assert!(init["capabilities"]["diagnosticProvider"].is_null(), "push mode without pull support");
    h.notify("initialized", json!({})).await;

    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": URI, "languageId": "lp", "version": 1, "text": TEXT } })).await;
    let first = h.diagnostics().await;
    assert!(codes(&first).contains(&"duplicate-name".to_owned()), "duplicate constraint name reported: {first:?}");

    // Rename the second `c1` (line 4, columns 1..3) to `c2`.
    h.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 3 } }, "text": "c2" }]
        }),
    )
    .await;
    let second = h.diagnostics().await;
    assert!(!codes(&second).contains(&"duplicate-name".to_owned()), "duplicate cleared after edit: {second:?}");

    let rename = h
        .request(
            "textDocument/rename",
            json!({ "textDocument": { "uri": URI }, "position": { "line": 1, "character": 9 }, "newName": "flow" }),
        )
        .await;
    let edits = rename["changes"][URI].as_array().expect("rename edits for the document");
    assert_eq!(edits.len(), 4, "x occurs in the objective, both constraints and the bound: {edits:?}");
    assert!(edits.iter().all(|e| e["newText"] == "flow"));

    let rejected = h
        .service
        .ready()
        .await
        .expect("service ready")
        .call(
            Request::build("textDocument/rename")
                .id(999)
                .params(json!({ "textDocument": { "uri": URI }, "position": { "line": 1, "character": 9 }, "newName": "9lives" }))
                .finish(),
        )
        .await
        .expect("service call")
        .expect("a response");
    assert!(rejected.into_parts().1.is_err(), "leading digit rejected");

    let formatting = h
        .request("textDocument/formatting", json!({ "textDocument": { "uri": URI }, "options": { "tabSize": 2, "insertSpaces": true } }))
        .await;
    let edits = formatting.as_array().expect("formatting edits");
    assert!(!edits.is_empty(), "messy spacing gets formatted");

    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

/// Names returned by `workspace/symbol` for `query`, with their URIs.
async fn symbols(h: &mut Harness, query: &str) -> Vec<(String, String)> {
    let found = h.request("workspace/symbol", json!({ "query": query })).await;
    found
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|s| s["name"] == query)
                .map(|s| (s["name"].as_str().unwrap_or("").to_owned(), s["location"]["uri"].as_str().unwrap_or("").to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// `workspace/symbol` matches for `query`, polling until one appears (the
/// index is filled in the background).
async fn eventual_symbols(h: &mut Harness, query: &str) -> Vec<(String, String)> {
    for _ in 0..100 {
        let found = symbols(h, query).await;
        if !found.is_empty() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Vec::new()
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_index_tracks_disk_not_buffers() {
    let dir = std::env::temp_dir().join(format!("lp-lsp-e2e-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("a.lp"), "min\n obj: x\nst\n c1: x >= 1\nend\n").unwrap();
    let root = format!("file://{}", dir.display());

    let mut h = Harness::start();
    h.request("initialize", json!({ "rootUri": root, "capabilities": {} })).await;
    h.notify("initialized", json!({})).await;
    let indexed = eventual_symbols(&mut h, "c1").await;
    assert_eq!(indexed.len(), 1, "a.lp indexed from disk: {indexed:?}");

    // The same file opened under another spelling of its URI is listed once.
    let spelt = format!("{root}/%61.lp");
    h.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": spelt, "languageId": "lp", "version": 1, "text": "min\n obj: x\nst\n c1: x >= 1\nend\n" } }),
    )
    .await;
    assert_eq!(symbols(&mut h, "c1").await.len(), 1, "open and indexed copies of one file are not both listed");

    // Edit without saving, then close: the index must go back to the disk text.
    h.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": spelt, "version": 2 },
            "contentChanges": [{ "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 3 } }, "text": "unsaved" }]
        }),
    )
    .await;
    h.notify("textDocument/didClose", json!({ "textDocument": { "uri": spelt } })).await;
    let back = eventual_symbols(&mut h, "c1").await;
    assert_eq!(back.len(), 1, "disk text re-indexed after close: {back:?}");
    assert_eq!(symbols(&mut h, "unsaved").await, Vec::new(), "unsaved buffer text is not indexed");

    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
    std::fs::remove_dir_all(&dir).unwrap();
}
