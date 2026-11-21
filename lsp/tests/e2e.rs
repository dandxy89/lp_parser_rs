//! End-to-end: drive the server in-process through the tower service.
//! initialize → didOpen → diagnostics → didChange → updated diagnostics →
//! rename → formatting → shutdown.

use std::fmt::Write as _;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use lp_lsp::Backend;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::{Error, Request, Response};

const URI: &str = "file:///tmp/e2e.lp";
const TEXT: &str = "Minimize\n obj:  3 x+2 y\nSubject To\n c1: x + y >= 2\n c1: x - y <= 8\nBounds\n x <= 10\nEnd\n";

struct Harness {
    service: LspService<Backend>,
    notifications: mpsc::UnboundedReceiver<(String, Value)>,
    next_id: i64,
}

impl Harness {
    fn start() -> Self {
        let (service, socket) = lp_lsp::service();
        let (tx, notifications) = mpsc::unbounded_channel();
        let (mut requests, mut responses) = socket.split();
        // Play the client: forward notifications and server requests, answer
        // the requests with `null`.
        tokio::spawn(async move {
            while let Some(request) = requests.next().await {
                let (method, id, params) = request.into_parts();
                if tx.send((method.into_owned(), params.unwrap_or(Value::Null))).is_err() {
                    break;
                }
                if let Some(id) = id
                    && responses.send(Response::from_ok(id, Value::Null)).await.is_err()
                {
                    break;
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
    /// Diagnostics published for `version`. Earlier versions are skipped: the
    /// semantic pass for an older version may publish after a later edit arrives.
    async fn diagnostics(&mut self, version: i64) -> Vec<Value> {
        let deadline = Duration::from_secs(10);
        loop {
            let (method, params) =
                tokio::time::timeout(deadline, self.notifications.recv()).await.expect("diagnostics in time").expect("channel open");
            if method == "textDocument/publishDiagnostics" && params["uri"] == URI && params["version"] == version {
                return params["diagnostics"].as_array().cloned().unwrap_or_default();
            }
        }
    }
}

const REFRESH: &str = "workspace/diagnostic/refresh";

impl Harness {
    /// Wait for a server message with `method`, skipping others.
    async fn expect(&mut self, method: &str) {
        let deadline = Duration::from_secs(10);
        loop {
            let (got, _) = tokio::time::timeout(deadline, self.notifications.recv()).await.expect("message in time").expect("channel open");
            if got == method {
                return;
            }
        }
    }

    /// Methods of the server messages received during `wait`.
    async fn received(&mut self, wait: Duration) -> Vec<String> {
        tokio::time::sleep(wait).await;
        let mut methods = Vec::new();
        while let Ok((method, _)) = self.notifications.try_recv() {
            methods.push(method);
        }
        methods
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
    for capability in ["callHierarchyProvider", "linkedEditingRangeProvider"] {
        assert_eq!(init["capabilities"][capability], true, "{capability}");
    }
    assert_eq!(init["capabilities"]["codeActionProvider"]["resolveProvider"], true);
    assert!(init["capabilities"]["workspace"]["fileOperations"]["didRename"].is_object());
    h.notify("initialized", json!({})).await;

    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": URI, "languageId": "lp", "version": 1, "text": TEXT } })).await;
    let first = h.diagnostics(1).await;
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
    let second = h.diagnostics(2).await;
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

    // Usage lenses come without their reference lists, filled in on resolve.
    assert_eq!(init["capabilities"]["codeLensProvider"]["resolveProvider"], true);
    let lenses = h.request("textDocument/codeLens", json!({ "textDocument": { "uri": URI } })).await;
    let unresolved = lenses.as_array().expect("lenses").iter().find(|l| l["command"].is_null()).expect("an unresolved usage lens").clone();
    let resolved = h.request("codeLens/resolve", unresolved).await;
    assert_eq!(resolved["command"]["command"], "lp.showReferences");
    assert!(resolved["command"]["arguments"][2].is_array(), "reference locations: {resolved}");

    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn pull_mode_refreshes_only_when_the_semantic_pass_lands() {
    let mut h = Harness::start();
    let capabilities = json!({ "textDocument": { "diagnostic": {} }, "workspace": { "diagnostics": { "refreshSupport": true } } });
    // A long debounce keeps the edit's semantic pass out of the observed window.
    let options = json!({ "lp": { "semantic": { "debounceMs": 60_000 } } });
    let init = h.request("initialize", json!({ "capabilities": capabilities, "initializationOptions": options })).await;
    assert!(init["capabilities"]["diagnosticProvider"].is_object(), "pull mode");
    h.notify("initialized", json!({})).await;

    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": URI, "languageId": "lp", "version": 1, "text": TEXT } })).await;
    // The semantic pass after opening changes the diagnostics without an edit.
    h.expect(REFRESH).await;

    // The client pulls after its own edits: no refresh per keystroke.
    for version in 2..6 {
        h.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version },
                "contentChanges": [{ "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 1 } }, "text": "d" }]
            }),
        )
        .await;
    }
    let received = h.received(Duration::from_millis(500)).await;
    assert!(!received.iter().any(|m| m == REFRESH), "no refresh on didChange: {received:?}");

    let pulled = h.request("textDocument/diagnostic", json!({ "textDocument": { "uri": URI } })).await;
    assert_eq!(pulled["kind"], "full");
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn burst_of_edits_publishes_in_order_up_to_the_latest() {
    let mut h = Harness::start();
    let options = json!({ "lp": { "semantic": { "debounceMs": 60_000 } } });
    h.request("initialize", json!({ "capabilities": { "textDocument": { "publishDiagnostics": {} } }, "initializationOptions": options }))
        .await;
    h.notify("initialized", json!({})).await;
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": URI, "languageId": "lp", "version": 1, "text": TEXT } })).await;

    let last = 40;
    for version in 2..=last {
        h.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version },
                "contentChanges": [{ "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 1 } }, "text": "d" }]
            }),
        )
        .await;
    }
    // Stale versions may be skipped, but never published after a newer one,
    // and the latest one always is.
    let mut published = Vec::new();
    while published.last() != Some(&last) {
        let (method, params) = tokio::time::timeout(Duration::from_secs(10), h.notifications.recv())
            .await
            .expect("diagnostics in time")
            .expect("channel open");
        if method == "textDocument/publishDiagnostics" {
            published.push(params["version"].as_i64().expect("versioned diagnostics"));
        }
    }
    assert!(published.windows(2).all(|w| w[0] < w[1]), "published out of order: {published:?}");
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

const MODEL_URI: &str = "file:///tmp/e2e-model.lp";
/// `c2` is `c1` scaled by 0.5, `c4` cannot hold given the bounds, `c5` has a
/// huge coefficient (a heuristic finding), and `x` is `c1`'s first variable.
const MODEL_TEXT: &str = "Minimize\n obj: x + y + z + w\nSubject To\n c1: 2 x + 4 y <= 100\n c2: x + 2 y <= 50\n c4: z + w >= 100\n c5: 1e12 x + y >= 1\nBounds\n z <= 50\n w <= 20\nEnd\n";

impl Harness {
    /// The first `publishDiagnostics` for `uri` at `version` satisfying `wanted`.
    async fn diagnostics_where(&mut self, uri: &str, version: i64, wanted: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Duration::from_secs(10);
        loop {
            let (method, params) = tokio::time::timeout(deadline, self.notifications.recv())
                .await
                .expect("matching diagnostics in time")
                .expect("channel open");
            if method == "textDocument/publishDiagnostics" && params["uri"] == uri && params["version"] == version {
                let found = params["diagnostics"].as_array().cloned().unwrap_or_default();
                if wanted(&found) {
                    return found;
                }
            }
        }
    }

    /// The last `publishDiagnostics` for `uri` at `version`, once none has
    /// arrived for `quiet`: the semantic pass for that version has landed.
    async fn settled_diagnostics(&mut self, uri: &str, version: i64, quiet: Duration) -> Vec<Value> {
        let mut last = None;
        loop {
            match tokio::time::timeout(if last.is_some() { quiet } else { Duration::from_secs(10) }, self.notifications.recv()).await {
                Ok(Some((method, params))) => {
                    if method == "textDocument/publishDiagnostics" && params["uri"] == uri && params["version"] == version {
                        last = Some(params["diagnostics"].as_array().cloned().unwrap_or_default());
                    }
                }
                Ok(None) => panic!("channel closed"),
                Err(_) => return last.expect("diagnostics published in time"),
            }
        }
    }
}

fn with_code<'a>(diagnostics: &'a [Value], code: &str) -> Vec<&'a Value> {
    diagnostics.iter().filter(|d| d["code"] == code).collect()
}

async fn open_model(options: Value) -> Harness {
    let mut h = Harness::start();
    h.request("initialize", json!({ "capabilities": { "textDocument": { "publishDiagnostics": {} } }, "initializationOptions": options }))
        .await;
    h.notify("initialized", json!({})).await;
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": MODEL_URI, "languageId": "lp", "version": 1, "text": MODEL_TEXT } }))
        .await;
    h
}

#[tokio::test(flavor = "multi_thread")]
async fn model_diagnostics_are_published_with_code_source_related_and_data() {
    let mut h = open_model(json!({ "lp": { "semantic": { "debounceMs": 0 } } })).await;
    let found = h.diagnostics_where(MODEL_URI, 1, |d| d.iter().any(|x| x["code"] == "proportional-row")).await;

    let proportional = with_code(&found, "proportional-row");
    assert_eq!(proportional.len(), 1, "{found:?}");
    let d = proportional[0];
    assert_eq!(d["source"], "lp-lsp");
    assert_eq!(d["severity"], 2, "warning");
    assert_eq!(d["range"]["start"]["line"], 4, "anchored at `c2`");
    assert_eq!(d["data"], json!({ "category": "mathematical", "certainty": "proven" }));
    assert_eq!(d["relatedInformation"][0]["location"]["range"]["start"]["line"], 3, "related `c1`");
    assert!(d["message"].as_str().is_some_and(|m| m.contains("'c1' scaled by 0.5")), "{d}");

    let infeasible = with_code(&found, "row-infeasible-by-bounds");
    assert_eq!(infeasible.len(), 1, "{found:?}");
    assert_eq!(infeasible[0]["severity"], 1, "error");

    let large = with_code(&found, "large-coefficient");
    assert_eq!(large.len(), 1);
    assert_eq!(large[0]["data"]["certainty"], "heuristic");
    // The retired prefix is gone.
    assert!(found.iter().all(|d| !d["code"].as_str().unwrap_or_default().starts_with("analysis/")), "{found:?}");
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_disable_categories_and_heuristics_and_override_severity() {
    let options = json!({ "lp": {
        "semantic": { "debounceMs": 0 },
        "analysis": {
            "categories": { "numerical": false },
            "severityOverrides": { "proportional-row": "hint", "row-infeasible-by-bounds": "off" }
        }
    } });
    let mut h = open_model(options).await;
    let found = h.diagnostics_where(MODEL_URI, 1, |d| d.iter().any(|x| x["code"] == "proportional-row")).await;
    assert_eq!(with_code(&found, "proportional-row")[0]["severity"], 4, "overridden to hint");
    assert!(with_code(&found, "row-infeasible-by-bounds").is_empty(), "switched off: {found:?}");
    assert!(with_code(&found, "large-coefficient").is_empty(), "numerical category off: {found:?}");
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn configuration_change_refilters_the_cached_analysis() {
    let mut h = open_model(json!({ "lp": { "semantic": { "debounceMs": 0 } } })).await;
    let before = h.diagnostics_where(MODEL_URI, 1, |d| d.iter().any(|x| x["code"] == "large-coefficient")).await;
    assert_eq!(with_code(&before, "proportional-row").len(), 1);

    h.notify("workspace/didChangeConfiguration", json!({ "settings": { "lp": { "analysis": { "heuristics": false } } } })).await;
    let after = h.diagnostics_where(MODEL_URI, 1, |d| !d.iter().any(|x| x["code"] == "large-coefficient")).await;
    assert!(with_code(&after, "proportional-row").len() == 1, "proven findings stay: {after:?}");

    h.notify("workspace/didChangeConfiguration", json!({ "settings": { "lp": { "analysis": { "heuristics": true } } } })).await;
    let restored = h.diagnostics_where(MODEL_URI, 1, |d| d.iter().any(|x| x["code"] == "large-coefficient")).await;
    assert_eq!(with_code(&restored, "proportional-row").len(), 1);
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_invalidates_the_cached_analysis() {
    let mut h = open_model(json!({ "lp": { "semantic": { "debounceMs": 0 } } })).await;
    h.diagnostics_where(MODEL_URI, 1, |d| d.iter().any(|x| x["code"] == "proportional-row")).await;

    // `c2: x + 2 y <= 50` becomes `x + 3 y`: no longer proportional to `c1`.
    h.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": MODEL_URI, "version": 2 },
            "contentChanges": [{ "range": { "start": { "line": 4, "character": 9 }, "end": { "line": 4, "character": 10 } }, "text": "3" }]
        }),
    )
    .await;
    let settled = h.settled_diagnostics(MODEL_URI, 2, Duration::from_millis(1500)).await;
    assert!(with_code(&settled, "proportional-row").is_empty(), "stale analysis dropped: {settled:?}");
    assert_eq!(with_code(&settled, "row-infeasible-by-bounds").len(), 1, "fresh analysis present: {settled:?}");
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_analysis_settings_are_rejected_with_a_message() {
    let mut h = Harness::start();
    h.request(
        "initialize",
        json!({ "capabilities": {}, "initializationOptions": { "lp": { "analysis": { "severityOverrides": { "not-a-code": "off" } } } } }),
    )
    .await;
    h.expect("window/showMessage").await;
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

// ---------------------------------------------------------------------------
// Custom `lp/*` requests.
// ---------------------------------------------------------------------------

const MODEL: &str = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 2\n c2: x - y <= 8\nBounds\n x <= 10\nEnd\n";
const BROKEN: &str = "Minimize\n obj: 3 x +\nSubject To\n c1: >= \nEnd\n";

/// A JSON-RPC error code of a failed request.
fn code(error: &Error) -> i64 {
    error.code.code()
}

fn data_kind(error: &Error) -> Option<String> {
    error.data.as_ref().and_then(|d| d["kind"].as_str()).map(str::to_owned)
}

impl Harness {
    /// Like [`Harness::request`] but returns the error instead of panicking.
    async fn call(&mut self, method: &'static str, params: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request::build(method).id(id).params(params).finish();
        let response = self.service.ready().await.expect("service ready").call(request).await.expect("service call").expect("a response");
        response.into_parts().1
    }

    /// Start a request without waiting for it; returns its id and the pending result.
    async fn spawn_call(&mut self, method: &'static str, params: Value) -> (i64, tokio::task::JoinHandle<Result<Value, Error>>) {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request::build(method).id(id).params(params).finish();
        let future = self.service.ready().await.expect("service ready").call(request);
        (id, tokio::spawn(async move { future.await.expect("service call").expect("a response").into_parts().1 }))
    }

    async fn cancel(&mut self, id: i64) {
        self.notify("$/cancelRequest", json!({ "id": id })).await;
    }

    /// Initialise, optionally with `lp` settings, and open `text` at `uri` (version 1).
    async fn open_with(&mut self, uri: &str, text: &str, lp: Value) -> Value {
        let init = self
            .request(
                "initialize",
                json!({ "capabilities": { "textDocument": { "publishDiagnostics": {} } }, "initializationOptions": { "lp": lp } }),
            )
            .await;
        self.notify("initialized", json!({})).await;
        self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "lp", "version": 1, "text": text } }))
            .await;
        init
    }

    /// `$/progress` values for `token` received so far, waiting for an `end`.
    async fn progress_until_end(&mut self, token: &str) -> Vec<Value> {
        let mut values = Vec::new();
        loop {
            let (method, params) = tokio::time::timeout(Duration::from_secs(10), self.notifications.recv())
                .await
                .expect("progress in time")
                .expect("channel open");
            if method == "$/progress" && params["token"] == token {
                let done = params["value"]["kind"] == "end";
                values.push(params["value"].clone());
                if done {
                    return values;
                }
            }
        }
    }

    /// Like [`Harness::progress_until_end`] for a numeric token.
    async fn progress_until_end_numeric(&mut self, token: i64) -> Vec<Value> {
        let mut values = Vec::new();
        loop {
            let (method, params) = tokio::time::timeout(Duration::from_secs(10), self.notifications.recv())
                .await
                .expect("progress in time")
                .expect("channel open");
            if method == "$/progress" && params["token"] == token {
                let done = params["value"]["kind"] == "end";
                values.push(params["value"].clone());
                if done {
                    return values;
                }
            }
        }
    }

    /// Server messages received during `wait`, with params.
    async fn drain(&mut self, wait: Duration) -> Vec<(String, Value)> {
        tokio::time::sleep(wait).await;
        let mut messages = Vec::new();
        while let Ok(message) = self.notifications.try_recv() {
            messages.push(message);
        }
        messages
    }

    /// Next server message with `method`, with its params.
    async fn next(&mut self, method: &str) -> Value {
        loop {
            let (got, params) = tokio::time::timeout(Duration::from_secs(10), self.notifications.recv())
                .await
                .expect("message in time")
                .expect("channel open");
            if got == method {
                return params;
            }
        }
    }
}

fn doc(uri: &str) -> Value {
    json!({ "uri": uri })
}

/// Settings that make the solver deterministically unavailable.
fn no_solver() -> Value {
    json!({ "solver": { "path": "/nonexistent/lp-highs" } })
}

const PROPORTIONAL: &str = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd\n";

fn model_findings(diagnostics: &[Value]) -> Vec<&Value> {
    diagnostics.iter().filter(|d| d["data"]["category"].is_string()).collect()
}

fn summary_total(model: &Value) -> usize {
    ["error", "warning", "info"]
        .iter()
        .map(|k| usize::try_from(model["summary"]["diagnostics"][*k].as_u64().expect("count")).expect("fits"))
        .sum()
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_model_and_lp_explain_agree_with_the_published_diagnostics() {
    let uri = "file:///tmp/agree.lp";
    let mut h = Harness::start();
    h.open_with(uri, PROPORTIONAL, json!({})).await;
    let published = h.diagnostics_where(uri, 1, |d| codes(d).iter().any(|c| c == "proportional-row")).await;
    let model = h.request("lp/model", json!({ "textDocument": doc(uri), "include": ["summary"] })).await;
    assert_eq!(summary_total(&model), model_findings(&published).len(), "{model} vs {published:?}");
    let c2 = h.request("lp/explain", json!({ "textDocument": doc(uri), "target": { "kind": "constraint", "name": "c2" } })).await;
    assert!(c2["diagnostics"].as_array().is_some_and(|d| d.contains(&json!("proportional-row"))), "{c2}");
}

#[tokio::test(flavor = "multi_thread")]
async fn severity_overrides_apply_to_lp_model_and_lp_explain_as_to_the_problems_panel() {
    let uri = "file:///tmp/overrides.lp";
    let mut h = Harness::start();
    h.open_with(uri, PROPORTIONAL, json!({ "analysis": { "severityOverrides": { "proportional-row": "off" } } })).await;
    let published = h.diagnostics_where(uri, 1, |_| true).await;
    assert!(!codes(&published).contains(&"proportional-row".to_owned()), "{published:?}");
    let model = h.request("lp/model", json!({ "textDocument": doc(uri), "include": ["summary"] })).await;
    assert_eq!(summary_total(&model), model_findings(&published).len(), "counts match the panel: {model} vs {published:?}");
    for name in ["c1", "c2"] {
        let explained =
            h.request("lp/explain", json!({ "textDocument": doc(uri), "target": { "kind": "constraint", "name": name } })).await;
        assert!(
            !explained["diagnostics"].as_array().is_some_and(|d| d.contains(&json!("proportional-row"))),
            "{name} must not report the hidden code: {explained}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_custom_requests_are_advertised() {
    let mut h = Harness::start();
    let init = h.open_with("file:///tmp/adv.lp", MODEL, no_solver()).await;
    let lp = &init["capabilities"]["experimental"]["lp"];
    assert_eq!(lp["protocol"], 1);
    assert_eq!(lp["requests"], json!(["lp/model", "lp/explain", "lp/graph", "lp/matrix", "lp/diff", "lp/solve", "lp/solverInfo"]));
    assert_eq!(lp["notifications"], json!(["lp/solutionChanged"]));
    assert_eq!(lp["solver"], false, "no helper at the configured path");
    assert_eq!(h.request("shutdown", Value::Null).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_model_describes_the_document_and_reports_its_version() {
    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;
    let model = h.request("lp/model", json!({ "textDocument": doc(URI) })).await;
    assert_eq!((model["uri"].clone(), model["version"].clone()), (json!(URI), json!(1)));
    assert_eq!(model["summary"]["constraints"], 2);
    assert_eq!(model["summary"]["variables"], 2);
    assert_eq!(model["constraints"][1]["name"], "c2");
    assert_eq!(model["variables"][0]["bounds"]["upper"], 10.0);

    // The result describes the buffer, so it follows edits and their versions.
    h.notify(
        "textDocument/didChange",
        json!({ "textDocument": { "uri": URI, "version": 2 }, "contentChanges": [{ "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 3 } }, "text": "c9" }] }),
    )
    .await;
    let first = h.request("lp/model", json!({ "textDocument": doc(URI), "include": ["constraints"], "limit": 1 })).await;
    assert_eq!(
        (first["version"].clone(), first["constraints"][0]["name"].clone(), first["truncated"]["constraints"].clone()),
        (json!(2), json!("c1"), json!(true))
    );
    let edited = h.request("lp/model", json!({ "textDocument": doc(URI), "include": ["constraints"], "offset": 1, "limit": 1 })).await;
    assert_eq!(
        (edited["version"].clone(), edited["constraints"][0]["name"].clone(), edited["truncated"]["constraints"].clone()),
        (json!(2), json!("c9"), json!(false))
    );
    assert!(edited.get("summary").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_model_error_paths() {
    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;
    let unknown = h.call("lp/model", json!({ "textDocument": doc("file:///tmp/never-opened.lp") })).await.expect_err("unknown document");
    assert_eq!(code(&unknown), -32602);
    let too_big = h.call("lp/model", json!({ "textDocument": doc(URI), "limit": 50_001 })).await.expect_err("limit cap");
    assert_eq!(code(&too_big), -32602);
    let bad_type = h.call("lp/model", json!({ "textDocument": doc(URI), "limit": "many" })).await.expect_err("wrong type");
    assert_eq!(code(&bad_type), -32602);
    let missing = h.call("lp/model", json!({})).await.expect_err("missing textDocument");
    assert_eq!(code(&missing), -32602);
    let bad_section =
        h.call("lp/model", json!({ "textDocument": doc(URI), "include": ["everything"] })).await.expect_err("unknown section");
    assert_eq!(code(&bad_section), -32602);

    let broken = "file:///tmp/broken.lp";
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": broken, "languageId": "lp", "version": 1, "text": BROKEN } })).await;
    let parse = h.call("lp/model", json!({ "textDocument": doc(broken) })).await.expect_err("parse error");
    assert_eq!((code(&parse), data_kind(&parse)), (-32803, Some("parseError".to_owned())));
    assert!(parse.data.as_ref().is_some_and(|d| d["message"].is_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_explain_by_name_and_by_position() {
    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;
    let by_name = h.request("lp/explain", json!({ "textDocument": doc(URI), "target": { "kind": "constraint", "name": "c1" } })).await;
    assert_eq!(
        (by_name["kind"].clone(), by_name["name"].clone(), by_name["operator"].clone()),
        (json!("constraint"), json!("c1"), json!(">="))
    );
    assert_eq!(by_name["terms"].as_array().map(Vec::len), Some(2));
    assert!(by_name["markdown"].as_str().is_some_and(|m| m.starts_with("### c1")));
    assert!(by_name.get("solution").is_none(), "nothing has been solved");

    // Line 1 is ` obj: 3 x + 2 y`: `x` is at column 9.
    let by_position = h.request("lp/explain", json!({ "textDocument": doc(URI), "position": { "line": 1, "character": 9 } })).await;
    assert_eq!((by_position["kind"].clone(), by_position["name"].clone()), (json!("variable"), json!("x")));
    assert_eq!(by_position["bounds"]["upper"], 10.0);

    let unknown = h
        .call("lp/explain", json!({ "textDocument": doc(URI), "target": { "kind": "variable", "name": "nope" } }))
        .await
        .expect_err("unknown");
    assert_eq!(code(&unknown), -32602);
    let neither = h.call("lp/explain", json!({ "textDocument": doc(URI) })).await.expect_err("no selector");
    assert_eq!(code(&neither), -32602);
    let bad_kind =
        h.call("lp/explain", json!({ "textDocument": doc(URI), "target": { "kind": "row", "name": "c1" } })).await.expect_err("bad kind");
    assert_eq!(code(&bad_kind), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_graph_and_matrix() {
    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;

    let graph = h.request("lp/graph", json!({ "textDocument": doc(URI) })).await;
    assert_eq!(graph["nodes"].as_array().map(Vec::len), Some(5), "obj, c1, c2, x, y");
    assert_eq!(graph["edges"]["source"].as_array().map(Vec::len), Some(6));
    assert_eq!(graph["components"], json!({ "count": 1, "sizes": [5] }));
    let focused = h.request("lp/graph", json!({ "textDocument": doc(URI), "focus": { "kind": "constraint", "name": "c1" } })).await;
    assert_eq!(focused["nodes"].as_array().map(Vec::len), Some(3), "c1 and its two variables");
    let capped = h.request("lp/graph", json!({ "textDocument": doc(URI), "maxNodes": 3 })).await;
    assert_eq!(capped["truncated"], true);
    for (params, label) in [
        (json!({ "textDocument": doc(URI), "maxNodes": 20_001 }), "node cap"),
        (json!({ "textDocument": doc(URI), "maxEdges": 200_001 }), "edge cap"),
        (json!({ "textDocument": doc(URI), "focus": { "kind": "variable", "name": "x", "depth": 4 } }), "depth"),
        (json!({ "textDocument": doc(URI), "focus": { "kind": "variable", "name": "nope" } }), "unknown focus"),
    ] {
        let error = h.call("lp/graph", params).await.expect_err(label);
        assert_eq!(code(&error), -32602, "{label}");
    }

    let entries = h.request("lp/matrix", json!({ "textDocument": doc(URI), "mode": "entries" })).await;
    assert_eq!(entries["shape"], json!({ "rows": 3, "cols": 2, "nonzeros": 6 }));
    assert_eq!(entries["entries"]["value"].as_array().map(Vec::len), Some(6));
    assert_eq!(entries["rows"][0], json!({ "name": "obj", "kind": "objective" }));
    let density = h.request("lp/matrix", json!({ "textDocument": doc(URI), "mode": "density", "bins": { "rows": 2, "cols": 2 } })).await;
    assert_eq!(density["bins"], json!({ "rows": 2, "cols": 2 }));
    assert_eq!(density["count"].as_array().map(|c| c.iter().filter_map(Value::as_u64).sum::<u64>()), Some(6));
    for (params, label) in [
        (json!({ "textDocument": doc(URI) }), "mode is required"),
        (json!({ "textDocument": doc(URI), "mode": "sparkline" }), "unknown mode"),
        (json!({ "textDocument": doc(URI), "mode": "entries", "rows": { "offset": 0, "limit": 5001 } }), "window cap"),
        (json!({ "textDocument": doc(URI), "mode": "density", "bins": { "rows": 513, "cols": 4 } }), "bins cap"),
    ] {
        let error = h.call("lp/matrix", params).await.expect_err(label);
        assert_eq!(code(&error), -32602, "{label}");
    }

    let broken = "file:///tmp/broken-graph.lp";
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": broken, "languageId": "lp", "version": 1, "text": BROKEN } })).await;
    for method in ["lp/graph", "lp/matrix"] {
        let error = h.call(method, json!({ "textDocument": doc(broken), "mode": "entries" })).await.expect_err("parse error");
        assert_eq!((code(&error), data_kind(&error)), (-32803, Some("parseError".to_owned())), "{method}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lp_diff_against_text_and_documents() {
    let dir = temp_dir("diff");
    let on_disk = dir.join("old.lp");
    std::fs::write(&on_disk, "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\nEnd\n").unwrap();
    let on_disk_uri = format!("file://{}", on_disk.display());

    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;

    let old = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\n gone: x <= 4\nEnd\n";
    let diff = h.request("lp/diff", json!({ "textDocument": doc(URI), "against": { "text": old, "label": "HEAD" } })).await;
    assert_eq!((diff["label"].clone(), diff["identical"].clone(), diff["version"].clone()), (json!("HEAD"), json!(false), json!(1)));
    assert_eq!(diff["constraints"]["added"][0]["name"], "c2");
    assert!(diff["constraints"]["added"][0]["range"]["start"].is_object());
    assert_eq!(diff["constraints"]["removed"], json!([{ "name": "gone" }]));
    assert_eq!(diff["constraints"]["modified"][0]["name"], "c1");

    let same = h.request("lp/diff", json!({ "textDocument": doc(URI), "against": { "text": MODEL } })).await;
    assert_eq!(same["identical"], true);
    let tolerant = h
        .request("lp/diff", json!({ "textDocument": doc(URI), "against": { "text": MODEL.replace("<= 8", "<= 8.001") }, "tolerance": { "abs": 0.01, "rel": 0 } }))
        .await;
    assert_eq!(tolerant["identical"], true);

    // `against.uri`: a file on disk, then an open document.
    let from_disk = h.request("lp/diff", json!({ "textDocument": doc(URI), "against": { "uri": on_disk_uri } })).await;
    assert_eq!(from_disk["label"], json!(on_disk_uri));
    assert_eq!(from_disk["constraints"]["added"][0]["name"], "c2");
    let other = "file:///tmp/other-open.lp";
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": other, "languageId": "lp", "version": 5, "text": old } })).await;
    let from_open = h.request("lp/diff", json!({ "textDocument": doc(URI), "against": { "uri": other } })).await;
    assert_eq!(from_open["constraints"]["removed"], json!([{ "name": "gone" }]));

    // Error paths.
    let negative = h
        .call("lp/diff", json!({ "textDocument": doc(URI), "against": { "text": old }, "tolerance": { "abs": -1 } }))
        .await
        .expect_err("tolerance");
    assert_eq!(code(&negative), -32602);
    let no_against = h.call("lp/diff", json!({ "textDocument": doc(URI) })).await.expect_err("against is required");
    assert_eq!(code(&no_against), -32602);
    let missing_file = h
        .call("lp/diff", json!({ "textDocument": doc(URI), "against": { "uri": "file:///tmp/definitely/not/here.lp" } }))
        .await
        .expect_err("missing file");
    assert_eq!(code(&missing_file), -32602);
    let bad_old = h.call("lp/diff", json!({ "textDocument": doc(URI), "against": { "text": BROKEN } })).await.expect_err("old side broken");
    assert_eq!((code(&bad_old), data_kind(&bad_old)), (-32803, Some("parseError".to_owned())));
    assert_eq!(bad_old.data.as_ref().map(|d| d["side"].clone()), Some(json!("against")));
    let broken = "file:///tmp/broken-diff.lp";
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": broken, "languageId": "lp", "version": 1, "text": BROKEN } })).await;
    let bad_new =
        h.call("lp/diff", json!({ "textDocument": doc(broken), "against": { "text": old } })).await.expect_err("current side broken");
    assert_eq!(bad_new.data.as_ref().map(|d| d["side"].clone()), Some(json!("current")));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 10 001 constraints: over the progress threshold.
fn big_model() -> String {
    let mut text = String::from("Minimize\n obj: y\nSubject To\n");
    for i in 0..=10_000 {
        writeln!(text, " c{i}: x{i} + y >= 1").unwrap();
    }
    text.push_str("End\n");
    text
}

#[tokio::test(flavor = "multi_thread")]
async fn big_requests_report_progress_and_small_ones_do_not() {
    let mut h = Harness::start();
    h.open_with(URI, &big_model(), no_solver()).await;
    let model = h.request("lp/model", json!({ "textDocument": doc(URI), "include": ["summary"], "workDoneToken": "big" })).await;
    assert_eq!(model["summary"]["constraints"], 10_001);
    let progress = h.progress_until_end("big").await;
    assert_eq!(progress[0]["kind"], "begin");
    assert_eq!(progress[0]["cancellable"], true, "the client may cancel");
    assert_eq!(progress.last().map(|p| p["kind"].clone()), Some(json!("end")));
    assert_eq!(progress.iter().filter(|p| p["kind"] == "begin").count(), 1, "one begin per request");

    h.request("lp/matrix", json!({ "textDocument": doc(URI), "mode": "density", "workDoneToken": 7 })).await;
    let matrix = h.progress_until_end_numeric(7).await;
    assert_eq!(matrix.first().map(|p| p["kind"].clone()), Some(json!("begin")));

    let small = "file:///tmp/small-progress.lp";
    h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": small, "languageId": "lp", "version": 1, "text": MODEL } })).await;
    h.request("lp/model", json!({ "textDocument": doc(small), "workDoneToken": "small" })).await;
    let seen = h.drain(Duration::from_millis(300)).await;
    assert!(
        !seen.iter().any(|(method, params)| method == "$/progress" && params["token"] == "small"),
        "a small model reports no progress: {seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn solve_without_a_helper_is_unavailable_and_the_rest_keeps_working() {
    let mut h = Harness::start();
    h.open_with(URI, MODEL, no_solver()).await;
    let error = h.call("lp/solve", json!({ "textDocument": doc(URI) })).await.expect_err("no helper");
    assert_eq!((code(&error), data_kind(&error)), (-32803, Some("solverUnavailable".to_owned())));
    let reason = error.data.as_ref().and_then(|d| d["reason"].as_str()).unwrap_or("").to_owned();
    assert!(reason.contains("/nonexistent/lp-highs"), "{reason}");
    let info = h.request("lp/solverInfo", json!({})).await;
    assert_eq!(info["available"], false);
    assert!(info["reason"].as_str().is_some_and(|r| r.contains("lp.solver.path")));
    // Everything else is unaffected.
    assert_eq!(h.request("lp/model", json!({ "textDocument": doc(URI) })).await["summary"]["constraints"], 2);
}

#[cfg(unix)]
mod fake_solver {
    //! `lp/solve` against stand-in helper scripts: protocol, errors,
    //! cancellation and progress without needing `HiGHS`.

    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;

    const OK_RESPONSE: &str = r#"{"protocol":1,"ok":true,"result":{"solver":{"name":"HiGHS","version":"1.11.0","helper":"lp-highs 0.1.0"},"status":"Optimal","objectiveValue":4.0,"timings":{"buildMs":1,"solveMs":2,"extractMs":0},"variables":[{"name":"x","value":0.0,"reducedCost":1.0},{"name":"y","value":2.0,"reducedCost":0.0}],"constraints":[{"name":"c1","activity":2.0,"dual":2.0,"slack":0.0,"binding":true},{"name":"c2","activity":-2.0,"dual":0.0,"slack":10.0,"binding":false}],"skipped":{"sos":0},"infeasibility":null,"log":null}}"#;

    const INFEASIBLE_RESPONSE: &str = r#"{"protocol":1,"ok":true,"result":{"solver":{"name":"HiGHS","version":"1.11.0","helper":"lp-highs 0.1.0"},"status":"Infeasible","objectiveValue":null,"timings":{"buildMs":1,"solveMs":2,"extractMs":0},"variables":[],"constraints":[],"skipped":{"sos":0},"infeasibility":{"method":"iis","constraints":[{"name":"c1","side":"lower"},{"name":"c2","side":"upper"}],"bounds":[{"variable":"x","side":"upper"}]},"log":null}}"#;

    const INFO: &str = r#"{"name":"lp-highs","version":"0.1.0","protocol":1,"highs":"1.11.0"}"#;

    /// A helper script: `info` prints [`INFO`], `solve` runs `solve_body`.
    fn helper(dir: &Path, solve_body: &str) -> PathBuf {
        helper_with_info(dir, INFO, solve_body)
    }

    fn helper_with_info(dir: &Path, info: &str, solve_body: &str) -> PathBuf {
        let path = dir.join("lp-highs");
        let script = format!("#!/bin/sh\ncase \"$1\" in\ninfo) echo '{info}' ;;\nsolve)\n{solve_body}\n;;\n*) exit 2 ;;\nesac\n");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Print `response` as the helper's answer, after recording its input.
    fn answering(dir: &Path, response: &str) -> String {
        format!(
            "cat > '{d}/request.json'\npwd > '{d}/cwd.txt'\necho '{{\"event\":\"phase\",\"phase\":\"building\",\"elapsedMs\":12}}' >&2\necho '{{\"event\":\"phase\",\"phase\":\"solving\",\"elapsedMs\":340}}' >&2\necho 'solver chatter' >&2\ncat <<'JSON'\n{response}\nJSON",
            d = dir.display()
        )
    }

    /// Records its pid, swallows stdin and sleeps: a solve that never ends.
    fn hanging(dir: &Path) -> String {
        format!("echo $$ > '{d}/pid'\ncat > /dev/null\nexec sleep 30", d = dir.display())
    }

    fn settings(helper: &Path) -> Value {
        json!({ "solver": { "path": helper.display().to_string() } })
    }

    fn model_uri(dir: &Path) -> String {
        format!("file://{}/model.lp", dir.display())
    }

    async fn wait_for(path: &Path) -> String {
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(path)
                && !text.trim().is_empty()
            {
                return text.trim().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("{} never appeared", path.display());
    }

    fn alive(pid: &str) -> bool {
        std::process::Command::new("kill").args(["-0", pid]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
    }

    async fn assert_gone(pid: &str) {
        for _ in 0..200 {
            if !alive(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("helper process {pid} is still running");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_solve_returns_the_solution_with_ranges_and_runs_beside_the_model() {
        let dir = temp_dir("solve-ok");
        let helper = helper(&dir, &answering(&dir, OK_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        let init = h.open_with(&uri, MODEL, settings(&helper)).await;
        assert_eq!(init["capabilities"]["experimental"]["lp"]["solver"], true, "found at startup");

        let info = h.request("lp/solverInfo", json!({})).await;
        assert_eq!(
            (info["available"].clone(), info["helperVersion"].clone(), info["highsVersion"].clone(), info["protocol"].clone()),
            (json!(true), json!("0.1.0"), json!("1.11.0"), json!(1))
        );

        let result = h
            .request(
                "lp/solve",
                json!({ "textDocument": doc(&uri), "options": { "mipRelGap": 0.01 }, "diagnose": "never", "workDoneToken": "s1" }),
            )
            .await;
        assert_eq!((result["uri"].clone(), result["version"].clone(), result["status"].clone()), (json!(uri), json!(1), json!("Optimal")));
        assert_eq!(result["objectiveValue"], 4.0);
        assert_eq!(result["solver"]["helper"], "lp-highs 0.1.0");
        assert_eq!(
            result["variables"][1],
            json!({ "name": "y", "value": 2.0, "reducedCost": 0.0, "range": result["variables"][1]["range"] })
        );
        assert!(result["variables"][1]["range"]["start"].is_object());
        assert!(result["constraints"][0]["range"]["start"]["line"].is_u64());
        assert_eq!(result["constraints"][1]["binding"], false);

        // The helper got the buffer text, our options and the model's folder as cwd.
        let request: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("request.json")).unwrap()).unwrap();
        assert_eq!(
            (request["protocol"].clone(), request["format"].clone(), request["text"].clone()),
            (json!(1), json!("lp"), json!(MODEL))
        );
        assert_eq!(
            (request["options"]["timeLimit"].clone(), request["options"]["mipRelGap"].clone(), request["diagnose"].clone()),
            (json!(60.0), json!(0.01), json!("never"))
        );
        assert_eq!(
            Path::new(std::fs::read_to_string(dir.join("cwd.txt")).unwrap().trim()).canonicalize().unwrap(),
            dir.canonicalize().unwrap()
        );

        // Phases become progress messages; the solution is announced.
        let progress = h.progress_until_end("s1").await;
        assert_eq!(progress[0]["kind"], "begin");
        let messages: Vec<&str> = progress.iter().filter_map(|p| p["message"].as_str()).collect();
        assert!(messages.iter().any(|m| m.starts_with("building")) && messages.iter().any(|m| m.starts_with("solving")), "{messages:?}");
        let changed = h.next("lp/solutionChanged").await;
        assert_eq!(changed, json!({ "uri": uri, "version": 1, "status": "Optimal" }));

        // The stored solution shows in explain, and goes stale on edit.
        let c1 = h.request("lp/explain", json!({ "textDocument": doc(&uri), "target": { "kind": "constraint", "name": "c1" } })).await;
        assert_eq!(c1["solution"], json!({ "stale": false, "activity": 2.0, "slack": 0.0, "binding": true, "dual": 2.0 }));
        h.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 3 } }, "text": "k1" }] }),
        )
        .await;
        let x = h.request("lp/explain", json!({ "textDocument": doc(&uri), "target": { "kind": "variable", "name": "x" } })).await;
        assert_eq!((x["solution"]["stale"].clone(), x["solution"]["value"].clone()), (json!(true), json!(0.0)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn infeasible_models_name_their_conflict_with_ranges() {
        let dir = temp_dir("solve-iis");
        let helper = helper(&dir, &answering(&dir, INFEASIBLE_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let result = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(result["status"], "Infeasible");
        let iis = &result["infeasibility"];
        assert_eq!(iis["method"], "iis");
        assert_eq!(iis["constraints"].as_array().map(Vec::len), Some(2));
        assert!(iis["constraints"][1]["range"]["start"]["line"].is_u64());
        assert!(iis["bounds"][0]["range"]["start"].is_object());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn position(line: u32, character: u32) -> Value {
        json!({ "line": line, "character": character })
    }

    async fn hover_text(h: &mut Harness, uri: &str, line: u32, character: u32) -> String {
        let hover = h.request("textDocument/hover", json!({ "textDocument": doc(uri), "position": position(line, character) })).await;
        hover["contents"]["value"].as_str().unwrap_or_default().to_owned()
    }

    async fn hint_labels(h: &mut Harness, uri: &str) -> Vec<String> {
        let range = json!({ "start": position(0, 0), "end": position(20, 0) });
        let hints = h.request("textDocument/inlayHint", json!({ "textDocument": doc(uri), "range": range })).await;
        hints.as_array().expect("hints").iter().filter_map(|hint| hint["label"].as_str().map(str::to_owned)).collect()
    }

    async fn solve_and_wait(h: &mut Harness, uri: &str) {
        h.request("lp/solve", json!({ "textDocument": doc(uri), "diagnose": "never" })).await;
        h.next("lp/solutionChanged").await;
    }

    async fn edit_name_of_c1(h: &mut Harness, uri: &str, version: i64) {
        h.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "range": { "start": { "line": 3, "character": 1 }, "end": { "line": 3, "character": 3 } }, "text": "k1" }] }),
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hover_and_inlay_hints_show_solution_values_until_the_next_edit() {
        let dir = temp_dir("solve-hover");
        let helper = helper(&dir, &answering(&dir, OK_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        assert!(!hover_text(&mut h, &uri, 3, 2).await.contains("Solution"), "nothing before a solve");
        assert!(hint_labels(&mut h, &uri).await.iter().all(|l| !l.starts_with("activity")));
        solve_and_wait(&mut h, &uri).await;

        let c1 = hover_text(&mut h, &uri, 3, 2).await;
        assert!(c1.contains("- **Solution:** activity 2, slack 0, dual 2, binding"), "{c1}");
        let y = hover_text(&mut h, &uri, 1, 14).await;
        assert!(y.contains("- **Solution:** 2, reduced cost 0"), "{y}");
        let objective = hover_text(&mut h, &uri, 1, 2).await;
        assert!(objective.contains("objective value 4"), "{objective}");
        let labels = hint_labels(&mut h, &uri).await;
        for expected in ["= 4", "= 0", "= 2", "activity 2", "activity -2"] {
            assert!(labels.iter().any(|l| l == expected), "{expected} in {labels:?}");
        }

        // The edit makes the solution stale: hidden by default.
        edit_name_of_c1(&mut h, &uri, 2).await;
        assert!(!hover_text(&mut h, &uri, 3, 2).await.contains("Solution"));
        assert!(!hover_text(&mut h, &uri, 1, 14).await.contains("Solution"));
        assert!(hint_labels(&mut h, &uri).await.iter().all(|l| !l.starts_with("activity") && !l.starts_with("= 2")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn show_stale_keeps_values_after_an_edit_and_marks_them() {
        let dir = temp_dir("solve-stale");
        let helper = helper(&dir, &answering(&dir, OK_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, json!({ "solver": { "path": helper.display().to_string() }, "solution": { "showStale": true } })).await;
        solve_and_wait(&mut h, &uri).await;
        edit_name_of_c1(&mut h, &uri, 2).await;
        let y = hover_text(&mut h, &uri, 1, 14).await;
        assert!(y.contains("- **Solution:** 2") && y.contains("(stale"), "{y}");
        assert!(hint_labels(&mut h, &uri).await.iter().any(|l| l == "= 2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_inlay_hints_setting_hides_values_but_not_the_hover() {
        let dir = temp_dir("solve-no-hints");
        let helper = helper(&dir, &answering(&dir, OK_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, json!({ "solver": { "path": helper.display().to_string() }, "solution": { "inlayHints": false } })).await;
        solve_and_wait(&mut h, &uri).await;
        assert!(hint_labels(&mut h, &uri).await.iter().all(|l| !l.starts_with("activity") && !l.starts_with("= ")));
        assert!(hover_text(&mut h, &uri, 1, 14).await.contains("- **Solution:** 2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn iis_members_are_published_as_diagnostics_until_the_next_edit() {
        let dir = temp_dir("solve-iis-diagnostics");
        let helper = helper(&dir, &answering(&dir, INFEASIBLE_RESPONSE));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let before = h.diagnostics_where(&uri, 1, |_| true).await;
        assert!(!codes(&before).contains(&"solver/iis-member".to_owned()), "{before:?}");
        h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;

        let members = h.diagnostics_where(&uri, 1, |d| codes(d).iter().any(|c| c == "solver/iis-member")).await;
        let members: Vec<&Value> = members.iter().filter(|d| d["code"] == "solver/iis-member").collect();
        let lines: Vec<u64> = members.iter().filter_map(|d| d["range"]["start"]["line"].as_u64()).collect();
        assert_eq!(lines, vec![3, 4, 6], "c1, c2 and the bound on x: {members:?}");
        assert!(members.iter().all(|d| d["data"]["category"] == "solver" && d["source"] == "lp-lsp"), "{members:?}");

        edit_name_of_c1(&mut h, &uri, 2).await;
        let after = h.diagnostics_where(&uri, 2, |_| true).await;
        assert!(!codes(&after).contains(&"solver/iis-member".to_owned()), "cleared by the edit: {after:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn solver_errors_map_to_their_kinds() {
        let dir = temp_dir("solve-errors");
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper(&dir, "cat > /dev/null\necho 'segfault in presolve' >&2\nexit 1"))).await;

        let crashed = h.call("lp/solve", json!({ "textDocument": doc(&uri) })).await.expect_err("crash");
        assert_eq!((code(&crashed), data_kind(&crashed)), (-32803, Some("solverFailed".to_owned())));
        assert!(
            crashed.data.as_ref().and_then(|d| d["stderrTail"].as_str()).is_some_and(|t| t.contains("segfault in presolve")),
            "{crashed:?}"
        );

        // Garbage on stdout, a failing exit and a well-formed error response.
        for (body, expected, label) in [
            ("cat > /dev/null\necho 'not json'", "solverFailed", "invalid JSON"),
            ("cat > /dev/null\necho '{\"protocol\":2,\"ok\":true}'", "solverFailed", "wrong protocol"),
            (
                "cat > /dev/null\necho '{\"protocol\":1,\"ok\":false,\"error\":{\"kind\":\"unsupportedModel\",\"message\":\"indicator constraints\"}}'",
                "unsupportedModel",
                "unsupported",
            ),
            (
                "cat > /dev/null\necho '{\"protocol\":1,\"ok\":false,\"error\":{\"kind\":\"parseError\",\"message\":\"bad\"}}'",
                "parseError",
                "helper parse error",
            ),
            (
                "cat > /dev/null\necho '{\"protocol\":1,\"ok\":false,\"error\":{\"kind\":\"protocol\",\"message\":\"bad request\"}}'\nexit 2",
                "solverFailed",
                "protocol error",
            ),
        ] {
            helper(&dir, body);
            let error = h.call("lp/solve", json!({ "textDocument": doc(&uri) })).await.expect_err(label);
            assert_eq!((code(&error), data_kind(&error)), (-32803, Some(expected.to_owned())), "{label}: {error:?}");
        }

        // The model is checked before the solver is run.
        helper(&dir, &answering(&dir, OK_RESPONSE));
        let broken = format!("file://{}/broken.lp", dir.display());
        h.notify("textDocument/didOpen", json!({ "textDocument": { "uri": broken, "languageId": "lp", "version": 1, "text": BROKEN } }))
            .await;
        let parse = h.call("lp/solve", json!({ "textDocument": doc(&broken) })).await.expect_err("parse error");
        assert_eq!((code(&parse), data_kind(&parse)), (-32803, Some("parseError".to_owned())));

        // Bad options are the caller's fault.
        for options in [json!({ "timeLimit": -5 }), json!({ "mipRelGap": -0.1 }), json!({ "threads": 0 }), json!({ "presolve": "maybe" })] {
            let error = h.call("lp/solve", json!({ "textDocument": doc(&uri), "options": options })).await.expect_err("bad option");
            assert_eq!(code(&error), -32602, "{options}");
        }
        let unknown =
            h.call("lp/solve", json!({ "textDocument": doc("file:///tmp/never-opened.lp") })).await.expect_err("unknown document");
        assert_eq!(code(&unknown), -32602);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_helper_with_another_protocol_is_unavailable() {
        let dir = temp_dir("solve-protocol");
        let helper = helper_with_info(&dir, r#"{"name":"lp-highs","version":"9.0.0","protocol":2,"highs":"2.0.0"}"#, "exit 2");
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        let init = h.open_with(&uri, MODEL, settings(&helper)).await;
        assert_eq!(init["capabilities"]["experimental"]["lp"]["solver"], false);
        let info = h.request("lp/solverInfo", json!({})).await;
        assert!(info["reason"].as_str().is_some_and(|r| r.contains("protocol 2") && r.contains("expects protocol 1")), "{info}");
        let error = h.call("lp/solve", json!({ "textDocument": doc(&uri) })).await.expect_err("mismatch");
        assert_eq!(data_kind(&error), Some("solverUnavailable".to_owned()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn solver_info_picks_up_a_helper_installed_later() {
        let dir = temp_dir("solve-later");
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        let path = dir.join("lp-highs");
        h.open_with(&uri, MODEL, json!({ "solver": { "path": path.display().to_string() } })).await;
        assert_eq!(h.request("lp/solverInfo", json!({})).await["available"], false);
        helper(&dir, &answering(&dir, OK_RESPONSE));
        assert_eq!(h.request("lp/solverInfo", json!({})).await["available"], true, "solverInfo re-probes");
        // `lp/solve` also re-probes before giving up.
        let result = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(result["status"], "Optimal");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_the_request_kills_the_helper() {
        let dir = temp_dir("solve-cancel");
        let helper = helper(&dir, &hanging(&dir));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let (id, pending) = h.spawn_call("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        let pid = wait_for(&dir.join("pid")).await;
        assert!(alive(&pid), "the helper is running");
        h.cancel(id).await;
        let error = pending.await.unwrap().expect_err("cancelled");
        assert_eq!(code(&error), -32800);
        assert_gone(&pid).await;
        // The server carries on.
        assert_eq!(h.request("lp/model", json!({ "textDocument": doc(&uri) })).await["summary"]["constraints"], 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_through_the_progress_token_kills_the_helper_and_ends_progress() {
        let dir = temp_dir("solve-progress-cancel");
        let helper = helper(&dir, &hanging(&dir));
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let (_, pending) = h.spawn_call("lp/solve", json!({ "textDocument": doc(&uri), "workDoneToken": "cancel-me" })).await;
        let pid = wait_for(&dir.join("pid")).await;
        h.notify("window/workDoneProgress/cancel", json!({ "token": "cancel-me" })).await;
        let error = pending.await.unwrap().expect_err("cancelled");
        assert_eq!(code(&error), -32800);
        assert_gone(&pid).await;
        let progress = h.progress_until_end("cancel-me").await;
        assert_eq!(progress[0]["kind"], "begin");
        assert_eq!(progress[0]["cancellable"], true);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_newer_solve_of_the_same_document_cancels_the_running_one() {
        let dir = temp_dir("solve-supersede");
        // The first run hangs, later runs answer.
        let body = format!(
            "if [ -f '{d}/started' ]; then\n{answer}\nelse\ntouch '{d}/started'\n{hang}\nfi",
            d = dir.display(),
            answer = answering(&dir, OK_RESPONSE),
            hang = hanging(&dir)
        );
        let helper = helper(&dir, &body);
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let (_, first) = h.spawn_call("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        let pid = wait_for(&dir.join("pid")).await;
        let second = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(second["status"], "Optimal");
        let error = first.await.unwrap().expect_err("superseded");
        assert_eq!(code(&error), -32800, "the earlier solve is cancelled");
        assert_gone(&pid).await;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closing_the_document_cancels_its_solve_and_forgets_its_solution() {
        let dir = temp_dir("solve-close");
        let body = format!(
            "if [ -f '{d}/started' ]; then\n{answer}\nelse\ntouch '{d}/started'\n{hang}\nfi",
            d = dir.display(),
            answer = answering(&dir, OK_RESPONSE),
            hang = hanging(&dir)
        );
        let helper = helper(&dir, &body);
        let uri = model_uri(&dir);
        let mut h = Harness::start();
        h.open_with(&uri, MODEL, settings(&helper)).await;
        let (_, running) = h.spawn_call("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        let pid = wait_for(&dir.join("pid")).await;
        h.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } })).await;
        assert_eq!(code(&running.await.unwrap().expect_err("closed")), -32800);
        assert_gone(&pid).await;
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// A fresh directory under the temp dir.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lp-lsp-e2e-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Solves with the real `lp-highs` helper. Needs `cargo build -p lp-highs`
/// first (the helper is not a dependency: lp-lsp never links `HiGHS`).
#[cfg(feature = "solver-tests")]
mod real_solver {
    use super::*;

    /// The built helper: `$LP_HIGHS`, else the nearest `lp-highs` in a parent
    /// of the test binary (`target/<profile>/`, wherever cargo puts the tests).
    fn helper() -> std::path::PathBuf {
        if let Some(path) = std::env::var_os("LP_HIGHS") {
            return path.into();
        }
        let name = if cfg!(windows) { "lp-highs.exe" } else { "lp-highs" };
        let exe = std::env::current_exe().unwrap();
        exe.ancestors().skip(1).map(|dir| dir.join(name)).find(|path| path.is_file()).unwrap_or_else(|| {
            panic!("no {name} above {}: run `cargo build -p lp-highs` before `cargo test -p lp-lsp --features solver-tests`", exe.display())
        })
    }

    async fn session(text: &str) -> (Harness, String, std::path::PathBuf) {
        let dir = temp_dir("real");
        let uri = format!("file://{}/model.lp", dir.display());
        let mut h = Harness::start();
        let init = h.open_with(&uri, text, json!({ "solver": { "path": helper().display().to_string() } })).await;
        assert_eq!(init["capabilities"]["experimental"]["lp"]["solver"], true);
        (h, uri, dir)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn solves_an_lp_with_duals() {
        let (mut h, uri, dir) = session(MODEL).await;
        let result = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(result["status"], "Optimal");
        assert!((result["objectiveValue"].as_f64().unwrap() - 4.0).abs() < 1e-6, "{result}");
        assert!(result["constraints"].as_array().unwrap().iter().all(|c| c["dual"].is_number()), "an LP has duals");
        assert_eq!(result["solver"]["name"], "HiGHS");
        let changed = h.next("lp/solutionChanged").await;
        assert_eq!(changed["status"], "Optimal");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_mip_has_no_duals() {
        let (mut h, uri, dir) =
            session("Maximize\n obj: x + y\nSubject To\n c1: 2 x + 3 y <= 12\nBounds\n x <= 4\nGenerals\n x y\nEnd\n").await;
        let result = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(result["status"], "Optimal");
        assert!(result["constraints"][0].get("dual").is_none(), "MIP duals mislead: {result}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_infeasible_model_is_diagnosed_with_an_iis() {
        let (mut h, uri, dir) = session("Minimize\n obj: x\nSubject To\n lo: x >= 5\n hi: x <= 3\nEnd\n").await;
        let result = h.request("lp/solve", json!({ "textDocument": doc(&uri) })).await;
        assert_eq!(result["status"], "Infeasible");
        assert_eq!(result["infeasibility"]["method"], "iis");
        let names: Vec<&str> =
            result["infeasibility"]["constraints"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
        assert!(names.contains(&"lo") && names.contains(&"hi"), "{result}");
        assert!(result["infeasibility"]["constraints"][0]["range"]["start"].is_object());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unsupported_models_are_reported() {
        let (mut h, uri, dir) = session("Minimize\n obj: x + y\nSubject To\n ind: b = 1 -> x + y <= 3\nBinaries\n b\nEnd\n").await;
        let error = h.call("lp/solve", json!({ "textDocument": doc(&uri) })).await.expect_err("indicator constraints are not supported");
        assert_eq!(data_kind(&error), Some("unsupportedModel".to_owned()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_a_real_solve_stops_the_helper() {
        // A knapsack large enough to keep HiGHS busy for a while.
        let mut text = String::from("Maximize\n obj: ");
        text.push_str(&(0..400).map(|i| format!("{} x{i}", 7 + (i * 13) % 29)).collect::<Vec<_>>().join(" + "));
        text.push_str("\nSubject To\n cap: ");
        text.push_str(&(0..400).map(|i| format!("{} x{i}", 11 + (i * 17) % 31)).collect::<Vec<_>>().join(" + "));
        text.push_str(" <= 2500\nBinaries\n");
        text.push_str(&(0..400).map(|i| format!("x{i}")).collect::<Vec<_>>().join(" "));
        text.push_str("\nEnd\n");
        let (mut h, uri, dir) = session(&text).await;
        let (id, pending) = h.spawn_call("lp/solve", json!({ "textDocument": doc(&uri), "options": { "timeLimit": 120 } })).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        h.cancel(id).await;
        let outcome = tokio::time::timeout(Duration::from_secs(10), pending).await.expect("cancel is prompt").unwrap();
        // Either it was still running (cancelled) or it already finished: never a hang.
        assert!(outcome.is_ok() || outcome.as_ref().is_err_and(|e| code(e) == -32800), "{outcome:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
