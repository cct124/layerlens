//! 真实回环 HTTP 与官方 MCP 客户端回归；核心服务真实运行，不冒充原生 WebView 验收。

use super::{
    config::Config,
    receipts::{self, Receipts},
    server, tools,
};
use crate::workspace::WorkspaceHost;
use base64::Engine as _;
use layerlens_core::workspace_contract::{WorkspaceAction, WorkspaceErrorCode};
use rmcp::{
    RoleClient, ServiceExt,
    model::*,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

type Client = RunningService<RoleClient, ClientConfig>;

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("layerlens-dev-mcp-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("test_temp_cleanup error={error}");
        }
    }
}

struct App {
    server: server::Running,
    host: Arc<WorkspaceHost>,
    instance: Arc<super::config::Instance>,
    endpoint: String,
    server_exit: Option<oneshot::Receiver<()>>,
    core_exit: Option<oneshot::Receiver<()>>,
    temporary: Option<Temporary>,
}
impl App {
    async fn start() -> Self {
        let temporary = Temporary::new();
        let sample_root = temporary.0.join("samples");
        let run_root = temporary.0.join("runs");
        fs::create_dir(&sample_root).unwrap();
        fs::create_dir(&run_root).unwrap();
        for name in ["bitmap-raw.psd", "bitmap-rle.psd"] {
            fs::copy(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../crates/layerlens-core/tests/fixtures/psd")
                    .join(name),
                sample_root.join(name),
            )
            .unwrap();
        }
        let instance = Arc::new(
            Config {
                sample_root,
                run_root,
                port: 0,
                visible: false,
                web_driver_url: None,
            }
            .prepare()
            .unwrap(),
        );
        let (core_done, core_exit) = oneshot::channel();
        let host = Arc::new(
            WorkspaceHost::start(
                Default::default(),
                |_| {},
                move |result| {
                    result.unwrap();
                    let _ = core_done.send(());
                },
            )
            .unwrap(),
        );
        let (server_done, server_exit) = oneshot::channel();
        let server = server::start(host.clone(), instance.clone(), move || {
            let _ = server_done.send(());
        })
        .await
        .unwrap();
        assert!(instance.directory.join("ready").exists());
        let connection: Value =
            serde_json::from_slice(&fs::read(instance.directory.join("connection.json")).unwrap())
                .unwrap();
        let endpoint = connection["endpoint"].as_str().unwrap().to_owned();
        assert_eq!(connection["instanceId"], instance.id);
        Self {
            server,
            host,
            instance,
            endpoint,
            server_exit: Some(server_exit),
            core_exit: Some(core_exit),
            temporary: Some(temporary),
        }
    }
    async fn client(&self) -> Client {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(self.endpoint.clone())
                .auth_header(&self.instance.token)
                .max_concurrent_requests(8),
        );
        let client = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("layerlens-regression", "1"),
        )
        .with_protocol_version(ProtocolVersion::V_2025_11_25);
        tokio::time::timeout(Duration::from_secs(10), client.serve(transport))
            .await
            .unwrap()
            .unwrap()
    }
    async fn finish(mut self) {
        self.server.stop();
        tokio::time::timeout(Duration::from_secs(10), self.server_exit.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.core_exit.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert!(self.server.stopped());
        assert!(!self.server.failed());
        assert!(!self.instance.directory.join("connection.json").exists());
        assert!(!self.instance.directory.join("ready").exists());
        assert!(self.instance.directory.join("stopped").exists());
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.server.stop();
        let (server, core, temporary) = (
            self.server_exit.take(),
            self.core_exit.take(),
            self.temporary.take(),
        );
        let host = self.host.clone();
        // 断言失败时也先结束后台，再释放其运行目录；不与 stopped 标记发布竞争。
        tauri::async_runtime::spawn(async move {
            if let Some(server) = server
                && let Err(error) = tokio::time::timeout(Duration::from_secs(10), server).await
            {
                eprintln!("test_server_cleanup error={error}");
            }
            host.stop();
            if let Some(core) = core
                && let Err(error) = tokio::time::timeout(Duration::from_secs(10), core).await
            {
                eprintln!("test_core_cleanup error={error}");
            }
            drop(temporary);
        });
    }
}

async fn call(client: &Client, name: &'static str, args: Value) -> CallToolResult {
    tokio::time::timeout(
        Duration::from_secs(10),
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}
fn body(result: CallToolResult) -> Value {
    let body = result.structured_content.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result.content[0].as_text().unwrap().text).unwrap(),
        body
    );
    body
}
fn ok(result: CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let body = body(result);
    assert_eq!(body["data"]["status"], "ok");
    assert!(body["userSelection"].get("sessionId").is_some(), "{body}");
    body
}
fn args(instance: &Value, request: Value, id: Option<&str>) -> Value {
    json!({"instanceId":instance["instanceId"],"sessionId":instance["sessionId"],"clientRequestId":id,"request":request})
}
fn workspace_request(action: Value) -> Value {
    json!({"protocolVersion":1,"action":action})
}
fn selection_request(instance: &Value, operation: Value) -> Value {
    json!({"protocolVersion":1,"sessionId":instance["sessionId"],"operation":operation})
}
async fn snapshot(client: &Client, info: &Value) -> Value {
    ok(call(
        client,
        "dev_workspace",
        args(info, workspace_request(json!({"kind":"snapshot"})), None),
    )
    .await)["data"]["value"]["snapshot"]
        .clone()
}
async fn wait(client: &Client, info: &Value, condition: Value) -> Value {
    let result = ok(call(
        client,
        "dev_wait",
        args(info, json!({"condition":condition,"timeoutMs":5000}), None),
    )
    .await);
    assert_eq!(result["data"]["value"]["satisfied"], true, "{result}");
    result["data"]["value"]["lastState"].clone()
}

#[tokio::test]
async fn sdk_round_trip_preserves_snapshots_replays_cleanup_and_isolates_reopened_documents() {
    let app = App::start().await;
    let before = fs::read(app.instance.sample_root.join("bitmap-raw.psd")).unwrap();
    let client = app.client().await;
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 8);
    assert!(
        tools
            .iter()
            .all(|t| t.output_schema.is_some() && t.input_schema["type"] == "object")
    );
    let info = ok(call(&client, "dev_instance", json!({})).await)["data"]["value"].clone();
    for (id, name, count) in [
        ("open-a", "bitmap-raw.psd", 1),
        ("open-b", "bitmap-rle.psd", 2),
    ] {
        let request = args(
            &info,
            workspace_request(json!({"kind":"open","path":name})),
            Some(id),
        );
        let first = ok(call(&client, "dev_workspace", request.clone()).await);
        assert_eq!(first["data"]["value"]["kind"], "accepted");
        assert_eq!(
            ok(call(&client, "dev_workspace", request).await)["data"],
            first["data"]
        );
        wait(
            &client,
            &info,
            json!({"kind":"documentCount","count":count}),
        )
        .await;
    }
    let state = snapshot(&client, &info).await;
    let a = state["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "bitmap-raw.psd")
        .unwrap()
        .clone();
    let b = state["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "bitmap-rle.psd")
        .unwrap()
        .clone();
    ok(call(
        &client,
        "dev_workspace",
        args(
            &info,
            workspace_request(json!({"kind":"activate","documentId":a["id"]})),
            Some("activate-a"),
        ),
    )
    .await);
    wait(
        &client,
        &info,
        json!({"kind":"previewReady","documentId":a["id"],"revision":a["revision"]}),
    )
    .await;
    let preview = call(
        &client,
        "dev_preview",
        args(
            &info,
            json!({"protocolVersion":1,"documentId":a["id"],"revision":a["revision"]}),
            None,
        ),
    )
    .await;
    let image = preview.content.iter().find_map(|c| c.as_image()).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .unwrap();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size()];
    let frame = decoder.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (4, 3));
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../crates/layerlens-core/tests/fixtures/psd/manifest.json"
    ))
    .unwrap();
    let expected = manifest["samples"][0]["composite"]["rgbaHex"]
        .as_str()
        .unwrap();
    let actual: String = pixels[..frame.buffer_size()]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(actual, expected);
    let state = snapshot(&client, &info).await;
    let selected = ok(call(&client,"dev_selection",args(&info,selection_request(&info,json!({"kind":"region","documentId":a["id"],"documentRevision":a["revision"],"expectedRevision":state["selection"]["selectionRevision"],"bounds":{"x":0,"y":0,"width":4,"height":3}})),Some("region-a"))).await);
    assert_eq!(
        selected["userSelection"]["content"]["layers"][0]["details"]["layer"]["name"],
        "negative-origin"
    );
    assert_eq!(
        selected["userSelection"]["content"]["snapshotId"],
        selected["data"]["value"]["snapshotId"]
    );
    let task = ok(call(&client,"dev_selection",args(&info,selection_request(&info,json!({"kind":"createTask","snapshotId":selected["data"]["value"]["snapshotId"],"requestId":"core-create-a","name":"fixed-A"})),Some("task-a"))).await)["data"]["value"]["task"].clone();
    ok(call(&client,"dev_selection",args(&info,selection_request(&info,json!({"kind":"region","documentId":a["id"],"documentRevision":a["revision"],"expectedRevision":selected["userSelection"]["selectionRevision"],"bounds":{"x":0,"y":1,"width":2,"height":1}})),Some("region-a-next"))).await);
    ok(call(
        &client,
        "dev_workspace",
        args(
            &info,
            workspace_request(json!({"kind":"activate","documentId":b["id"]})),
            Some("activate-b"),
        ),
    )
    .await);
    ok(call(
        &client,
        "dev_workspace",
        args(
            &info,
            workspace_request(json!({"kind":"close","documentId":a["id"]})),
            Some("close-a"),
        ),
    )
    .await);
    let read_task = args(
        &info,
        selection_request(
            &info,
            json!({"kind":"content","target":{"kind":"task","taskId":task["id"]},"cursor":null,"limit":32}),
        ),
        None,
    );
    let content = ok(call(&client, "dev_selection", read_task.clone()).await);
    assert_eq!(
        content["data"]["value"]["page"]["snapshotId"],
        task["scope"]["snapshotId"]
    );
    assert_eq!(
        content["data"]["value"]["page"]["documentRevision"],
        a["revision"]
    );
    assert_eq!(content["userSelection"]["documentId"], b["id"]);
    ok(call(
        &client,
        "dev_workspace",
        args(
            &info,
            workspace_request(json!({"kind":"open","path":"bitmap-raw.psd"})),
            Some("reopen-a"),
        ),
    )
    .await);
    let reopened = wait(&client, &info, json!({"kind":"documentCount","count":2})).await;
    let fresh = reopened["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "bitmap-raw.psd")
        .unwrap();
    assert_ne!(fresh["id"], a["id"]);
    let prepare = selection_request(
        &info,
        json!({"kind":"prepare","target":{"kind":"task","taskId":task["id"]}}),
    );
    let plan = ok(call(
        &client,
        "dev_cleanup",
        args(&info, prepare.clone(), Some("preflight-cancel")),
    )
    .await)["data"]["value"]["plan"]
        .clone();
    ok(call(
        &client,
        "dev_cleanup",
        args(
            &info,
            selection_request(&info, json!({"kind":"cancel","planId":plan["planId"]})),
            Some("cancel-plan"),
        ),
    )
    .await);
    ok(call(&client, "dev_selection", read_task.clone()).await);
    let plan = ok(call(
        &client,
        "dev_cleanup",
        args(&info, prepare, Some("preflight-confirm")),
    )
    .await)["data"]["value"]["plan"]
        .clone();
    assert_eq!(plan["documentId"], a["id"]);
    let confirm = args(
        &info,
        selection_request(&info, json!({"kind":"confirm","planId":plan["planId"]})),
        Some("confirm-old-a"),
    );
    let receipt = ok(call(&client, "dev_cleanup", confirm.clone()).await);
    assert_eq!(
        receipt["data"]["value"]["receipt"]["invalidatedTaskIds"],
        json!([task["id"]])
    );
    assert_eq!(
        ok(call(&client, "dev_cleanup", confirm).await)["data"],
        receipt["data"]
    );
    let failed = body(call(&client, "dev_selection", read_task).await);
    assert_eq!(failed["data"]["error"]["code"], "TASK_INVALIDATED");
    let conflict = body(
        call(
            &client,
            "dev_workspace",
            args(
                &info,
                workspace_request(json!({"kind":"close","documentId":b["id"]})),
                Some("open-a"),
            ),
        )
        .await,
    );
    assert_eq!(conflict["data"]["error"]["code"], "REQUEST_CONFLICT");
    let core = app.host.action(WorkspaceAction::Snapshot {}).await.unwrap();
    assert_eq!(core.documents.len(), 2);
    assert!(
        core.documents
            .iter()
            .any(|d| d.id == b["id"].as_str().unwrap())
    );
    assert!(
        core.documents
            .iter()
            .any(|d| d.id == fresh["id"].as_str().unwrap())
    );
    assert_eq!(
        fs::read(app.instance.sample_root.join("bitmap-raw.psd")).unwrap(),
        before
    );
    ok(call(&client, "dev_shutdown", args(&info, json!({}), None)).await);
    client.cancel().await.unwrap();
    app.finish().await;
}

async fn raw_status(
    app: &App,
    authorization: Option<&str>,
    host: &str,
    origin: Option<&str>,
    content: &str,
) -> u16 {
    let address = app
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n",
        content.len()
    );
    if let Some(token) = authorization {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(origin) = origin {
        request.push_str(&format!("Origin: {origin}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(content);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(bytes)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn authentication_host_origin_request_limit_and_foreign_identity_reject_before_work() {
    let app = App::start().await;
    let host = app
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    assert_eq!(raw_status(&app, None, host, None, "{}").await, 401);
    assert_eq!(raw_status(&app, Some("wrong"), host, None, "{}").await, 401);
    assert_eq!(
        raw_status(
            &app,
            Some(&app.instance.token),
            host,
            Some("http://untrusted.example"),
            "{}"
        )
        .await,
        403
    );
    assert_eq!(
        raw_status(
            &app,
            Some(&app.instance.token),
            "untrusted.example",
            None,
            "{}"
        )
        .await,
        403
    );
    assert_eq!(
        raw_status(
            &app,
            Some(&app.instance.token),
            host,
            None,
            &" ".repeat(tools::REQUEST_BYTES + 1)
        )
        .await,
        413
    );
    let client = app.client().await;
    let info = ok(call(&client, "dev_instance", json!({})).await)["data"]["value"].clone();
    let mut foreign = args(
        &info,
        workspace_request(json!({"kind":"open","path":"bitmap-raw.psd"})),
        Some("foreign"),
    );
    foreign["instanceId"] = json!("another-instance");
    assert!(
        client
            .call_tool(
                CallToolRequestParams::new("dev_workspace")
                    .with_arguments(foreign.as_object().unwrap().clone())
            )
            .await
            .is_err()
    );
    let invalid = body(
        call(
            &client,
            "dev_workspace",
            args(
                &info,
                workspace_request(json!({"kind":"open","path":"../outside.psd"})),
                Some("outside"),
            ),
        )
        .await,
    );
    assert_eq!(invalid["data"]["error"]["code"], "INVALID_INPUT");
    let missing = body(
        call(
            &client,
            "dev_workspace",
            args(
                &info,
                workspace_request(json!({"kind":"open","path":"bitmap-raw.psd"})),
                None,
            ),
        )
        .await,
    );
    assert_eq!(missing["data"]["error"]["code"], "INVALID_INPUT");
    assert!(
        snapshot(&client, &info).await["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.cancel().await.unwrap();
    app.finish().await;
}

#[tokio::test]
async fn wait_is_event_driven_does_not_block_actions_and_returns_last_state_on_timeout() {
    let app = App::start().await;
    let client = app.client().await;
    let info = ok(call(&client, "dev_instance", json!({})).await)["data"]["value"].clone();
    let waiting = client.call_tool(
        CallToolRequestParams::new("dev_wait").with_arguments(
            args(
                &info,
                json!({"condition":{"kind":"documentCount","count":1},"timeoutMs":5000}),
                None,
            )
            .as_object()
            .unwrap()
            .clone(),
        ),
    );
    let open = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let observed = ok(call(&client, "dev_instance", json!({})).await);
                if observed["data"]["value"]["activeWaits"] == 1 {
                    break;
                }
            }
        })
        .await
        .unwrap();
        ok(call(
            &client,
            "dev_workspace",
            args(
                &info,
                workspace_request(json!({"kind":"open","path":"bitmap-raw.psd"})),
                Some("open-while-waiting"),
            ),
        )
        .await)
    };
    let (waited, _) = tokio::join!(waiting, open);
    assert_eq!(ok(waited.unwrap())["data"]["value"]["satisfied"], true);
    let timeout = ok(call(
        &client,
        "dev_wait",
        args(
            &info,
            json!({"condition":{"kind":"documentCount","count":2},"timeoutMs":1}),
            None,
        ),
    )
    .await);
    assert_eq!(timeout["data"]["value"]["timedOut"], true);
    assert_eq!(
        timeout["data"]["value"]["lastState"]["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let invalid = body(
        call(
            &client,
            "dev_wait",
            args(
                &info,
                json!({"condition":{"kind":"sequence","atLeast":"00"},"timeoutMs":1}),
                None,
            ),
        )
        .await,
    );
    assert_eq!(invalid["data"]["error"]["code"], "INVALID_INPUT");
    client.cancel().await.unwrap();
    app.finish().await;
}

#[tokio::test]
async fn disconnected_waits_release_capacity_and_leave_business_calls_available() {
    let app = App::start().await;
    let client = app.client().await;
    let info = ok(call(&client, "dev_instance", json!({})).await)["data"]["value"].clone();
    let address = app
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let mut sockets = Vec::new();
    for id in 0..tools::WAITS {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let body = json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "dev_wait", "arguments": args(&info, json!({
                "condition": {"kind": "documentCount", "count": 8}, "timeoutMs": 30000
            }), None) }
        })
        .to_string();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{body}",
            app.instance.token,
            body.len()
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        sockets.push(socket);
    }
    async fn wait_for_count(client: &Client, expected: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let observed = ok(call(client, "dev_instance", json!({})).await);
                if observed["data"]["value"]["activeWaits"] == expected {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    wait_for_count(&client, tools::WAITS).await;
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("dev_wait").with_arguments(
                args(
                    &info,
                    json!({"condition":{"kind":"idle"},"timeoutMs":1}),
                    None,
                )
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await;
    assert!(rejected.is_err());
    // 等待者名额耗尽不占用工作区执行名额；断连无需等到 30 秒截止时间。
    assert_eq!(snapshot(&client, &info).await["documents"], json!([]));
    drop(sockets);
    wait_for_count(&client, 0).await;
    client.cancel().await.unwrap();
    app.finish().await;
}

#[tokio::test]
async fn mutation_receipt_survives_dropped_waiter_and_rejects_conflict_and_capacity() {
    let ledger = Receipts::default();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let (release, gate) = oneshot::channel();
    let first = ledger
        .start("request", vec![1], async move {
            observed.fetch_add(1, Ordering::SeqCst);
            gate.await.unwrap();
            Ok(json!(7))
        })
        .unwrap();
    drop(first);
    let duplicate = ledger
        .start("request", vec![1], async {
            panic!("duplicate must not execute");
        })
        .unwrap();
    assert_eq!(
        ledger
            .start("request", vec![2], async { Ok(json!(8)) })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::RequestConflict
    );
    release.send(()).unwrap();
    assert_eq!(Receipts::receive(duplicate).await.unwrap(), json!(7));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    for index in 1..receipts::RECORD_LIMIT {
        let reply = ledger
            .start(&format!("record-{index}"), vec![0], async { Ok(json!(1)) })
            .unwrap();
        Receipts::receive(reply).await.unwrap();
    }
    assert_eq!(
        ledger
            .start("overflow", vec![0], async { Ok(json!(0)) })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ResourceLimit
    );
    assert_eq!(
        Receipts::receive(
            ledger
                .start("request", vec![1], async { Ok(json!(9)) })
                .unwrap()
        )
        .await
        .unwrap(),
        json!(7)
    );
    ledger.finish().await;
    assert_eq!(
        ledger
            .start("after-shutdown", vec![0], async {
                panic!("关闭后不能执行新操作")
            })
            .unwrap_err()
            .code,
        WorkspaceErrorCode::ShuttingDown
    );
}

#[test]
fn configuration_and_paths_are_isolated_and_cannot_escape_samples() {
    use super::config::Instance;
    assert!(
        Instance::from_arguments(std::iter::empty())
            .unwrap()
            .is_none()
    );
    assert!(Instance::from_arguments(["--dev-mcp-config".into()].into_iter()).is_err());
    assert!(Instance::from_arguments(["--dev-mcp-unknown".into()].into_iter()).is_err());
    let temp = Temporary::new();
    let samples = temp.0.join("samples");
    let runs = temp.0.join("runs");
    fs::create_dir(&samples).unwrap();
    fs::create_dir(&runs).unwrap();
    fs::write(samples.join("input.psd"), b"fixture").unwrap();
    fs::write(temp.0.join("outside.psd"), b"outside").unwrap();
    let make = || {
        Config {
            sample_root: samples.clone(),
            run_root: runs.clone(),
            port: 0,
            visible: false,
            web_driver_url: None,
        }
        .prepare()
        .unwrap()
    };
    let a = make();
    let b = make();
    assert_ne!(a.id, b.id);
    assert_ne!(a.token, b.token);
    assert_ne!(a.directory, b.directory);
    // 模拟连接文件已写完、ready 发布失败；释放实例仍需回收凭据。
    let failed = make();
    let directory = failed.directory.clone();
    fs::write(directory.join("ready"), b"existing").unwrap();
    assert!(failed.publish("http://127.0.0.1:1/mcp").is_err());
    assert!(directory.join("connection.json").exists());
    drop(failed);
    assert!(!directory.join("connection.json").exists());
    assert!(!directory.join("ready").exists());
    assert!(a.sample("input.psd").unwrap().starts_with(&a.sample_root));
    assert!(a.sample("../outside.psd").is_err());
    assert!(
        a.sample(temp.0.join("outside.psd").to_str().unwrap())
            .is_err()
    );
    assert!(a.sample(".").is_err());
    assert!(
        Config {
            sample_root: samples.clone(),
            run_root: samples,
            port: 0,
            visible: false,
            web_driver_url: None,
        }
        .prepare()
        .is_err()
    );
}
