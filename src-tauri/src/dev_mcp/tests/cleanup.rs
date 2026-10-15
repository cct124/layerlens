//! 使用真实 MCP 客户端验证批量部分结果、满额清理、计划时钟和在途 PNG 发布。
use super::*;

async fn setup(client: &Client) -> (Value, Value, Value) {
    let info = ok(call(client, "dev_instance", json!({})).await)["data"]["value"].clone();
    ok(call(
        client,
        "dev_workspace",
        args(
            &info,
            workspace_request(json!({"kind":"open","path":"bitmap-raw.psd"})),
            Some("open"),
        ),
    )
    .await);
    let state = wait(client, &info, json!({"kind":"documentCount","count":1})).await;
    let document = state["documents"][0].clone();
    wait(
        client,
        &info,
        json!({"kind":"previewReady","documentId":document["id"],"revision":document["revision"]}),
    )
    .await;
    let selected = ok(call(client, "dev_selection", args(&info, selection_request(&info, json!({"kind":"region","documentId":document["id"],"documentRevision":document["revision"],"expectedRevision":state["selection"]["selectionRevision"],"bounds":{"x":0,"y":0,"width":4,"height":3}})), Some("region"))).await)["data"]["value"].clone();
    (info, document, selected["snapshotId"].clone())
}
async fn cleanup(client: &Client, info: &Value, operation: Value, id: &str) -> Value {
    body(
        call(
            client,
            "dev_cleanup",
            args(info, selection_request(info, operation), Some(id)),
        )
        .await,
    )["data"]
        .clone()
}

#[tokio::test]
async fn batch_partial_replay_and_expired_plan_keep_default_limits() {
    let app = App::start().await;
    let client = app.client().await;
    let (info, document, snapshot_id) = setup(&client).await;
    // 此服务测试没有前端；核心 PNG 就绪绝不能满足前端条件。
    let missing_frontend = ok(call(&client, "dev_wait", args(&info, json!({"condition":{"kind":"frontendImageReady","documentId":document["id"],"revision":document["revision"],"atLeast":"0"},"timeoutMs":1}), None)).await);
    assert_eq!(missing_frontend["data"]["value"]["satisfied"], false);
    assert_eq!(info["limits"]["mutationRecords"], 128);
    for (count, id) in [(0, "bad-zero"), (129, "bad-large")] {
        let result = body(
            call(
                &client,
                "dev_prepare_tasks",
                args(
                    &info,
                    json!({"snapshotId":snapshot_id,"count":count,"namePrefix":"测试"}),
                    Some(id),
                ),
            )
            .await,
        );
        assert_eq!(result["data"]["error"]["code"], "INVALID_INPUT");
    }
    ok(call(
        &client,
        "dev_prepare_tasks",
        args(
            &info,
            json!({"snapshotId":snapshot_id,"count":1,"namePrefix":"已有任务"}),
            Some("first"),
        ),
    )
    .await);
    let request = args(
        &info,
        json!({"snapshotId":snapshot_id,"count":128,"namePrefix":"\u{1}".repeat(240)}),
        Some("batch"),
    );
    let batch = ok(call(&client, "dev_prepare_tasks", request.clone()).await)["data"].clone();
    assert_eq!(batch["value"]["status"], "partial");
    assert_eq!(batch["value"]["taskIds"].as_array().unwrap().len(), 127);
    assert_eq!(batch["value"]["error"]["code"], "RESOURCE_LIMIT");
    assert_eq!(
        ok(call(&client, "dev_prepare_tasks", request).await)["data"],
        batch
    );
    let prepare = json!({"kind":"prepare","target":{"kind":"document","documentId":document["id"],"documentRevision":document["revision"]}});
    let plan = cleanup(&client, &info, prepare.clone(), "prepare").await;
    assert_eq!(
        plan["value"]["plan"]["tasks"].as_array().unwrap().len(),
        128
    );
    let clock = args(
        &info,
        json!({"kind":"advancePlanClock","seconds":301}),
        Some("clock"),
    );
    let advanced = ok(call(&client, "dev_test_control", clock.clone()).await)["data"].clone();
    assert_eq!(
        ok(call(&client, "dev_test_control", clock).await)["data"],
        advanced
    );
    let expired = cleanup(
        &client,
        &info,
        json!({"kind":"confirm","planId":plan["value"]["plan"]["planId"]}),
        "expired",
    )
    .await;
    assert_eq!(expired["error"]["code"], "CLEANUP_PLAN_EXPIRED");
    assert_eq!(
        snapshot(&client, &info).await["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let plan = cleanup(&client, &info, prepare, "new-plan").await;
    let confirmed = cleanup(
        &client,
        &info,
        json!({"kind":"confirm","planId":plan["value"]["plan"]["planId"]}),
        "confirm",
    )
    .await;
    assert_eq!(
        confirmed["value"]["receipt"]["invalidatedTaskIds"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
    ok(call(&client, "dev_shutdown", args(&info, json!({}), None)).await);
    client.cancel().await.unwrap();
    app.finish().await;
}

#[tokio::test]
async fn held_preview_keeps_accounting_until_cleanup_rejects_publication() {
    let app = App::start().await;
    let client = app.client().await;
    let (info, document, _) = setup(&client).await;
    let gate = ok(call(&client, "dev_test_control", args(&info, json!({"kind":"arm","checkpoint":"previewPublication","mode":"pause","timeoutMs":10000}), Some("arm"))).await)["data"]["value"]["gate"].clone();
    let read = call(
        &client,
        "dev_preview",
        args(
            &info,
            json!({"protocolVersion":1,"documentId":document["id"],"revision":document["revision"]}),
            None,
        ),
    );
    let control = async {
        let reached = ok(call(
            &client,
            "dev_test_control",
            args(
                &info,
                json!({"kind":"wait","id":gate["id"],"phase":"reached","timeoutMs":5000}),
                None,
            ),
        )
        .await);
        assert_eq!(reached["data"]["value"]["satisfied"], true);
        let plan = cleanup(&client, &info, json!({"kind":"prepare","target":{"kind":"document","documentId":document["id"],"documentRevision":document["revision"]}}), "prepare").await;
        let confirmed = cleanup(
            &client,
            &info,
            json!({"kind":"confirm","planId":plan["value"]["plan"]["planId"]}),
            "confirm",
        )
        .await;
        assert_eq!(confirmed["status"], "ok");
        let state = snapshot(&client, &info).await;
        assert!(
            state["resources"]["outputBytes"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        ok(call(
            &client,
            "dev_test_control",
            args(&info, json!({"kind":"release","id":gate["id"]}), None),
        )
        .await);
    };
    let (response, ()) = tokio::join!(read, control);
    assert_eq!(response.is_error, Some(true));
    assert!(!response.content.iter().any(|c| c.as_image().is_some()));
    assert_eq!(
        snapshot(&client, &info).await["resources"]["outputBytes"],
        "0"
    );
    ok(call(&client, "dev_shutdown", args(&info, json!({}), None)).await);
    client.cancel().await.unwrap();
    app.finish().await;
}
