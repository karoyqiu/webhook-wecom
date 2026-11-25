mod support;
use serde_json::json;
use support::{Harness, push_fixture};

#[test]
fn atlas_rejects_missing_signature_before_parsing_or_delivery() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    assert_eq!(ntfy_request(&app, "/atlas", b"{", &[]), 401);
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn atlas_open_uses_raw_body_signature_and_delivers_alert_details() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let body = br#"{"status":"OPEN","eventTypeName":"OUTSIDE_METRIC_THRESHOLD","clusterName":"cluster-0","humanReadable":"Disk usage high","groupId":"5d1b6f8e8c2e4e2d3c4a5b6d"}"#;
    // Signature independently calculated with .NET HMACSHA1 over these exact bytes.
    assert_eq!(
        ntfy_request(
            &app,
            "/atlas",
            body,
            &[("X-MMS-Signature", "ZZVCixwJ1kZTLH8Mks1fLD3A08Q=")]
        ),
        204
    );
    let messages = app.messages();
    assert_eq!(
        messages[0]["text"]["content"],
        "[Atlas OPEN] OUTSIDE_METRIC_THRESHOLD（cluster-0）\nDisk usage high\nhttps://cloud.mongodb.com/v2/5d1b6f8e8c2e4e2d3c4a5b6d#/alerts"
    );
    assert_eq!(messages[0]["touser"], "roy|jane");
    assert_eq!(messages[0]["toparty"], "2|3");
    assert_eq!(messages[0]["totag"], "4");
}

fn atlas_request(app: &Harness, body: &[u8]) -> u16 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(b"atlas-secret").unwrap();
    mac.update(body);
    ntfy_request(
        app,
        "/atlas",
        body,
        &[(
            "X-MMS-Signature",
            &STANDARD.encode(mac.finalize().into_bytes()),
        )],
    )
}

#[test]
fn atlas_malformed_json_and_supported_field_shapes_return_400_without_delivery() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for body in [
        "{",
        "[]",
        "null",
        "42",
        "\"alert\"",
        r#"{"status":3}"#,
        r#"{"clusterName":{}}"#,
        r#"{"eventTypeName":[]}"#,
        r#"{"humanReadable":true}"#,
        r#"{"groupId":5}"#,
    ] {
        assert_eq!(atlas_request(&app, body.as_bytes()), 400, "{body}");
    }
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn atlas_closed_and_missing_or_null_fields_still_deliver_without_empty_link() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for (payload, expected) in [
        (
            json!({"status":"CLOSED", "eventTypeName":"NO_PRIMARY", "clusterName":"cluster-0", "humanReadable":"Primary restored", "groupId":"project-id"}),
            "[Atlas CLOSED] NO_PRIMARY（cluster-0）\nPrimary restored\nhttps://cloud.mongodb.com/v2/project-id#/alerts",
        ),
        (
            json!({"status":"OPEN", "eventTypeName":"BILLING", "humanReadable":"Check billing"}),
            "[Atlas OPEN] BILLING（未知集群）\nCheck billing",
        ),
        (
            json!({}),
            "[Atlas 未知状态] 未知事件（未知集群）\n无告警描述",
        ),
        (
            json!({"status":null,"eventTypeName":null,"clusterName":null,"humanReadable":null,"groupId":null}),
            "[Atlas 未知状态] 未知事件（未知集群）\n无告警描述",
        ),
    ] {
        assert_eq!(atlas_request(&app, payload.to_string().as_bytes()), 204);
        assert_eq!(app.messages().last().unwrap()["text"]["content"], expected);
    }
}

#[test]
fn atlas_signature_errors_and_disabled_source_never_contact_downstream() {
    let disabled = Harness::start(None, vec![]);
    assert_eq!(atlas_request(&disabled, b"{}"), 404);
    assert!(disabled.received.lock().unwrap().is_empty());
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for signature in [
        "",
        "bad",
        "!!!!!!!!!!!!!!!!!!!!!!!!!!!!",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "AAAAAAAAAAAAAAAAAAAAAAAAAA==",
        "ZZVCixwJ1kZTLH8Mks1fLD3A08Q=",
    ] {
        assert_eq!(
            ntfy_request(&app, "/atlas", b"{", &[("X-MMS-Signature", signature)]),
            401,
            "{signature}"
        );
    }
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn atlas_shared_limits_truncate_unicode_body_and_report_delivery_failure() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload = json!({"status":"OPEN","eventTypeName":"NO_PRIMARY","clusterName":"cluster-0","humanReadable":"中文🦀".repeat(1000),"groupId":"project-id"});
    assert_eq!(atlas_request(&app, payload.to_string().as_bytes()), 204);
    let messages = app.messages();
    let text = messages[0]["text"]["content"].as_str().unwrap();
    assert!(text.starts_with("[Atlas OPEN] NO_PRIMARY（cluster-0）\n中文🦀"));
    assert!(text.ends_with("…\nhttps://cloud.mongodb.com/v2/project-id#/alerts"));
    assert!(text.len() <= 2048 && text.len() > 2040);
    payload["eventTypeName"] = json!("x".repeat(2048));
    assert_eq!(atlas_request(&app, payload.to_string().as_bytes()), 400);
    assert_eq!(atlas_request(&app, &vec![b'x'; 1024 * 1024 + 1]), 413);
    assert_eq!(app.messages().len(), 1);
    let failed = Harness::start(Some("gitea-secret"), vec![json!({"errcode":45009})]);
    assert_eq!(atlas_request(&failed, b"{}"), 502);
}

#[test]
fn atlas_logs_allowlisted_event_and_status_without_payload_values() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    assert_eq!(
        ntfy_request(
            &app,
            "/atlas",
            b"private payload",
            &[("X-MMS-Event", "alert.open")]
        ),
        401
    );
    assert_eq!(
        ntfy_request(
            &app,
            "/atlas",
            b"private payload",
            &[("X-MMS-Event", "private-event")]
        ),
        401
    );
    assert_eq!(
        String::from_utf8(app.logs.0.lock().unwrap().clone()).unwrap(),
        "source=atlas event=alert.open result=401\nsource=atlas event=unknown result=401\n"
    );
}

#[test]
fn token_errors_and_malformed_or_http_error_responses_fail_delivery() {
    for (status, body) in [
        (503, "unavailable"),
        (200, "not json"),
        (200, "{}"),
        (200, "{\"errcode\":40001}"),
        (
            200,
            "{\"errcode\":0,\"access_token\":\"\",\"expires_in\":7200}",
        ),
        (
            200,
            "{\"errcode\":0,\"access_token\":\"token\",\"expires_in\":0}",
        ),
    ] {
        let app = Harness::start_with(Some("gitea-secret"), move |_| (status, body.into()));
        assert_eq!(app.push(&push_fixture()), 502);
        assert!(app.messages().is_empty());
    }
    for (status, body) in [(503, "unavailable"), (200, "not json")] {
        let app = Harness::start_with(Some("gitea-secret"), move |path| {
            if path.starts_with("/cgi-bin/gettoken?") {
                (
                    200,
                    json!({"errcode":0,"access_token":"token","expires_in":7200}).to_string(),
                )
            } else {
                (status, body.into())
            }
        });
        assert_eq!(app.push(&push_fixture()), 502);
        assert_eq!(app.messages().len(), 1);
    }
}

#[test]
fn token_expires_before_next_delivery_and_query_credentials_are_encoded() {
    let app = Harness::start_with(Some("gitea-secret"), |path| {
        if path.starts_with("/cgi-bin/gettoken?") {
            assert_eq!(
                path,
                "/cgi-bin/gettoken?corpid=corp%20%26%3Fprivate&corpsecret=wecom-secret%26%3F"
            );
            (
                200,
                json!({"errcode":0,"access_token":"token","expires_in":1}).to_string(),
            )
        } else {
            (200, json!({"errcode":0}).to_string())
        }
    });
    assert_eq!(app.push(&push_fixture()), 204);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(app.push(&push_fixture()), 204);
    assert_eq!(
        app.received
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with("/cgi-bin/gettoken?"))
            .count(),
        2
    );
}

#[test]
fn push_uses_first_five_first_lines_author_fallbacks_and_tag_branch() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let payload = json!({"ref":"refs/tags/v1.0", "repository":{"full_name":"owner/repo"}, "commits":[
        {"message":"  first\nsecond line", "author":{"name":"Name","username":"ignored"}},
        {"message":"second", "author":{"username":"login"}},
        {"message":"third"}, {"message":"fourth"}, {"message":"fifth"}, {"message":"omitted"}
    ]});
    assert_eq!(app.push(&payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "[owner/repo] 6 次提交 -> v1.0\n- first (Name)\n- second (login)\n- third (未知作者)\n- fourth (未知作者)\n- fifth (未知作者)"
    );
}

#[test]
fn rendering_preserves_title_link_and_utf8_while_truncating_only_body() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload = json!({"ref":"refs/heads/main", "repository":{"full_name":"owner/repo"}, "commits":[{"message":"中文🦀".repeat(1000),"author":{"name":"作者"}}], "compare_url":"https://example.com/compare/abc...def"});
    assert_eq!(app.push(&payload), 204);
    let messages = app.messages();
    let text = messages[0]["text"]["content"].as_str().unwrap();
    assert!(text.starts_with("[owner/repo] 1 次提交 -> main\n- 中文🦀中文🦀"));
    assert!(text.ends_with("…\nhttps://example.com/compare/abc...def"));
    assert!(text.len() <= 2048 && text.len() > 2044);
    assert!(!text.contains('\u{fffd}'));
    payload.as_object_mut().unwrap().remove("compare_url");
    assert_eq!(app.push(&payload), 204);
    assert!(
        app.messages()[1]["text"]["content"]
            .as_str()
            .unwrap()
            .ends_with('…')
    );
}

#[test]
fn exact_byte_limit_is_unmodified_and_unrepresentable_envelopes_are_rejected() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    // 32 title bytes + newline + 2015 body bytes = 2048 bytes.
    let payload = json!({"ref":"refs/heads/main", "repository":{"full_name":"owner/repo"}, "commits":[{"message":"x".repeat(2007),"author":{"name":"roy"}}]});
    assert_eq!(app.push(&payload), 204);
    let messages = app.messages();
    let text = messages[0]["text"]["content"].as_str().unwrap();
    assert_eq!(text.len(), 2048);
    assert!(!text.contains('…'));
    for remaining in 0..3 {
        let mut payload = push_fixture();
        payload["compare_url"] = json!(
            "x".repeat(2048 - "[karoyqiu/gitea-ntfy] 2 次提交 -> main\n\n".len() - remaining)
        );
        assert_eq!(app.push(&payload), 400);
    }
    let mut payload = push_fixture();
    payload["repository"]["full_name"] = json!("x".repeat(2048));
    assert_eq!(app.push(&payload), 400);
    let mut payload = push_fixture();
    payload["compare_url"] = json!("x".repeat(2048));
    assert_eq!(app.push(&payload), 400);
    assert_eq!(app.messages().len(), 1);
}

#[test]
fn source_credentials_fail_closed_without_contacting_wecom() {
    let disabled = Harness::start(None, vec![]);
    assert_eq!(disabled.push(&push_fixture()), 404);
    assert!(disabled.received.lock().unwrap().is_empty());
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let body = push_fixture().to_string();
    for signature in [
        None,
        Some("incorrect"),
        Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
        Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"),
    ] {
        assert_eq!(
            app.request("/gitea", body.as_bytes(), signature, "push"),
            401
        );
    }
    assert_eq!(
        app.request(
            "/gitea",
            b"{}",
            Some(&support::signature(body.as_bytes())),
            "push"
        ),
        401
    );
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn malformed_json_and_non_object_payloads_return_400() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for body in [b"{".as_slice(), b"null", b"[]", b"\xff"] {
        assert_eq!(
            app.request("/gitea", body, Some(&support::signature(body)), "push"),
            400
        );
    }
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn request_body_limit_accepts_one_mib_and_rejects_larger_bodies() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut body = vec![b' '; 1024 * 1024];
    body[..2].copy_from_slice(b"{}");
    assert_eq!(
        app.request("/gitea", &body, Some(&support::signature(&body)), "push"),
        204
    );
    body.push(b' ');
    assert_eq!(
        app.request("/gitea", &body, Some(&support::signature(&body)), "push"),
        413
    );
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(3)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let response = agent
        .post(format!("{}/gitea", app.url))
        .header("X-Gitea-Event", "push")
        .header("X-Gitea-Signature", support::signature(&body))
        .send(ureq::SendBody::from_reader(&mut std::io::Cursor::new(body)))
        .unwrap();
    assert_eq!(response.status(), 413);
    assert_eq!(app.messages().len(), 1);
}

#[test]
fn downstream_failures_return_502_and_only_token_errors_retry() {
    for code in [40014, 42001, 40001, 42007, 42009, 45009] {
        let app = Harness::start(Some("gitea-secret"), vec![json!({"errcode":code}); 3]);
        assert_eq!(app.push(&push_fixture()), 502);
        let expected = if matches!(code, 40014 | 42001) { 2 } else { 1 };
        assert_eq!(app.messages().len(), expected);
    }
    let app = Harness::start(
        Some("gitea-secret"),
        vec![json!({"errmsg":"missing errcode"})],
    );
    assert_eq!(app.push(&push_fixture()), 502);
    assert_eq!(app.messages().len(), 1);
}

#[test]
fn logs_have_one_safe_line_per_request_without_secrets_or_untrusted_fields() {
    let app = Harness::start(
        Some("gitea-secret"),
        vec![json!({"errcode":45009,"errmsg":"wecom-secret&? access-private-1"})],
    );
    assert_eq!(app.push(&push_fixture()), 502);
    assert_eq!(
        app.request(
            "/private-path?token=private-query",
            b"",
            None,
            "private-event"
        ),
        404
    );
    assert_eq!(app.request("/gitea", b"{}", None, "private-event"), 401);
    assert_eq!(
        ureq::get(format!("{}/health", app.url))
            .call()
            .unwrap()
            .status(),
        200
    );
    let logs = String::from_utf8(app.logs.0.lock().unwrap().clone()).unwrap();
    assert_eq!(
        logs,
        "source=gitea event=push result=502\nsource=unknown event=unknown result=404\nsource=gitea event=unknown result=401\nsource=health event=unknown result=200\n"
    );
}

#[test]
fn invalid_access_token_refreshes_once_and_cached_token_is_reused() {
    for code in [40014, 42001] {
        let app = Harness::start(
            Some("gitea-secret"),
            vec![
                serde_json::json!({"errcode":code}),
                serde_json::json!({"errcode":0}),
            ],
        );
        assert_eq!(app.push(&push_fixture()), 204);
        assert_eq!(app.push(&push_fixture()), 204);
        let received = app.received.lock().unwrap();
        assert_eq!(
            received
                .iter()
                .filter(|(path, _)| path.starts_with("/cgi-bin/gettoken?"))
                .count(),
            2
        );
        let sends: Vec<_> = received
            .iter()
            .filter(|(path, _)| path.starts_with("/cgi-bin/message/send?"))
            .map(|(path, _)| path.as_str())
            .collect();
        assert_eq!(
            sends,
            [
                "/cgi-bin/message/send?access_token=access-private-1",
                "/cgi-bin/message/send?access_token=access-private-2",
                "/cgi-bin/message/send?access_token=access-private-2"
            ]
        );
    }
}

#[test]
fn signed_push_delivers_plain_text_and_configured_recipients() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    assert_eq!(app.push(&push_fixture()), 204);
    assert_eq!(
        app.messages(),
        vec![serde_json::json!({
            "touser":"roy|jane", "toparty":"2|3", "totag":"4", "agentid":1000002, "msgtype":"text",
            "text":{"content":"[karoyqiu/gitea-ntfy] 2 次提交 -> main\n- fix: handle empty payload (roy)\n- feat: add push (roy)\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/compare/abc...def"}
        })]
    );
}

#[test]
fn health_is_available_on_configured_listener() {
    let app = Harness::start(None, vec![]);
    assert_eq!(
        ureq::get(format!("{}/health", app.url))
            .call()
            .unwrap()
            .status(),
        200
    );
}

fn deliver_event(app: &Harness, event: &str, payload: &serde_json::Value) -> u16 {
    let body = payload.to_string();
    app.request(
        "/gitea",
        body.as_bytes(),
        Some(&support::signature(body.as_bytes())),
        event,
    )
}

#[test]
fn pull_requests_preserve_source_title_author_and_link_and_mark_closure() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pull_request_opened.json")).unwrap();
    assert_eq!(deliver_event(&app, "pull_request", &payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "[karoyqiu/gitea-ntfy] PR #42 已创建: Add webhook support\nAdd webhook support，作者：roy\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/pulls/42"
    );
    payload =
        serde_json::from_str(include_str!("fixtures/pull_request_closed_not_merged.json")).unwrap();
    assert_eq!(deliver_event(&app, "pull_request", &payload), 204);
    assert!(
        app.messages()[1]["text"]["content"]
            .as_str()
            .unwrap()
            .starts_with("🔀 [karoyqiu/gitea-ntfy] PR #42 已关闭:")
    );
    payload["action"] = json!("reopened");
    assert_eq!(deliver_event(&app, "pull_request", &payload), 204);
    assert!(
        app.messages()[2]["text"]["content"]
            .as_str()
            .unwrap()
            .starts_with("[karoyqiu/gitea-ntfy] PR #42 已重新打开:")
    );
}

#[test]
fn issues_preserve_number_action_source_title_author_and_link() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let payload: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/issues_opened.json")).unwrap();
    assert_eq!(deliver_event(&app, "issues", &payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "[karoyqiu/gitea-ntfy] 议题 #7 已创建: App crashes\nApp crashes，作者：roy\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/issues/7"
    );
}

#[test]
fn comments_preserve_commenter_and_link_and_truncate_at_300_unicode_characters() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/issue_comment_created.json")).unwrap();
    assert_eq!(deliver_event(&app, "issue_comment", &payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "[karoyqiu/gitea-ntfy] #7 的评论\nroy 评论：\nI can reproduce this.\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/issues/7#issuecomment-1"
    );
    payload["comment"]["body"] = json!("中🦀".repeat(150));
    assert_eq!(deliver_event(&app, "issue_comment", &payload), 204);
    assert!(
        !app.messages()[1]["text"]["content"]
            .as_str()
            .unwrap()
            .contains('…')
    );
    payload["comment"]["body"] = json!(format!("{}omit", "中🦀".repeat(150)));
    assert_eq!(deliver_event(&app, "issue_comment", &payload), 204);
    let expected = format!(
        "[karoyqiu/gitea-ntfy] #7 的评论\nroy 评论：\n{}…\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/issues/7#issuecomment-1",
        "中🦀".repeat(150)
    );
    assert_eq!(app.messages()[2]["text"]["content"], expected);
}

#[test]
fn releases_mark_priority_preserve_tag_name_and_link_and_truncate_notes() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/release_published.json")).unwrap();
    assert_eq!(deliver_event(&app, "release", &payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "🚀 [karoyqiu/gitea-ntfy] 发布 已发布: v1.2.0\nRelease v1.2.0\nBug fixes\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/releases/tag/v1.2.0"
    );
    payload["release"]["body"] = json!(format!("{}omit", "界".repeat(300)));
    assert_eq!(deliver_event(&app, "release", &payload), 204);
    assert_eq!(
        app.messages()[1]["text"]["content"],
        format!(
            "🚀 [karoyqiu/gitea-ntfy] 发布 已发布: v1.2.0\nRelease v1.2.0\n{}…\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/releases/tag/v1.2.0",
            "界".repeat(300)
        )
    );
}

#[test]
fn workflow_runs_show_result_marker_name_number_branch_trigger_and_link() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let success: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/workflow_run_success.json")).unwrap();
    assert_eq!(deliver_event(&app, "workflow_run", &success), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "✅ [karoyqiu/gitea-ntfy] CI: CI #42 成功\n在 main 上运行“CI”（由 push 触发）-> 成功\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/actions/runs/42"
    );
    let mut failure: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/workflow_run_failure.json")).unwrap();
    assert_eq!(deliver_event(&app, "workflow_run", &failure), 204);
    assert_eq!(
        app.messages()[1]["text"]["content"],
        "❌ [karoyqiu/gitea-ntfy] CI: CI #43 失败\n在 main 上运行“CI”（由 push 触发）-> 失败\nhttps://gitea.example.com/karoyqiu/gitea-ntfy/actions/runs/43"
    );
    failure["workflow_run"]["conclusion"] = json!("cancelled");
    assert_eq!(deliver_event(&app, "workflow_run", &failure), 204);
    assert!(
        app.messages()[2]["text"]["content"]
            .as_str()
            .unwrap()
            .starts_with("⚙️ [karoyqiu/gitea-ntfy] CI: CI #43 已取消\n")
    );
}

#[test]
fn unknown_events_deliver_repository_event_actor_without_link_or_unsafe_log_fields() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let mut payload = json!({"repository":{"full_name":"owner/repo"},"sender":{"login":"actor"},"pusher":{"login":"ignored"}});
    assert_eq!(deliver_event(&app, "star", &payload), 204);
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "[owner/repo] star 事件\nactor 触发了 star"
    );
    payload.as_object_mut().unwrap().remove("sender");
    assert_eq!(deliver_event(&app, "private-event", &payload), 204);
    assert_eq!(
        app.messages()[1]["text"]["content"],
        "[owner/repo] private-event 事件\nignored 触发了 private-event"
    );
    assert_eq!(
        String::from_utf8(app.logs.0.lock().unwrap().clone()).unwrap(),
        "source=gitea event=unknown result=204\nsource=gitea event=unknown result=204\n"
    );
}

#[test]
fn all_gitea_events_accept_missing_optional_fields_and_log_known_event_names() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for (event, expected) in [
        (
            "pull_request",
            "[未知仓库] PR #? 已更新: \n，作者：未知作者",
        ),
        ("issues", "[未知仓库] 议题 #? 已更新: \n，作者：未知作者"),
        ("issue_comment", "[未知仓库] #? 的评论\n未知评论者 评论：\n"),
        ("release", "🚀 [未知仓库] 发布 已更新: \n\n"),
        (
            "workflow_run",
            "⚙️ [未知仓库] CI: 工作流 #? 未知\n在 ? 上运行“工作流”（由 ? 触发）-> 未知",
        ),
    ] {
        assert_eq!(deliver_event(&app, event, &json!({})), 204);
        assert_eq!(app.messages().last().unwrap()["text"]["content"], expected);
    }
    assert_eq!(
        String::from_utf8(app.logs.0.lock().unwrap().clone()).unwrap(),
        "source=gitea event=pull_request result=204\nsource=gitea event=issues result=204\nsource=gitea event=issue_comment result=204\nsource=gitea event=release result=204\nsource=gitea event=workflow_run result=204\n"
    );
}

#[test]
fn gitea_action_and_workflow_status_labels_use_chinese() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for (action, label) in [
        ("edited", "已编辑"),
        ("assigned", "已指派"),
        ("unassigned", "已取消指派"),
        ("label_updated", "标签已更新"),
        ("label_cleared", "标签已清除"),
        ("synchronized", "已同步"),
        ("created", "已创建"),
        ("deleted", "已删除"),
        ("prereleased", "已预发布"),
        ("released", "已正式发布"),
        ("reviewed", "已评审"),
        ("review_requested", "已请求评审"),
        ("review_request_removed", "已取消评审请求"),
        ("ready_for_review", "已准备评审"),
        ("converted_to_draft", "已转为草稿"),
    ] {
        assert_eq!(
            deliver_event(&app, "issues", &json!({"action":action})),
            204
        );
        assert_eq!(
            app.messages().last().unwrap()["text"]["content"],
            format!("[未知仓库] 议题 #? {label}: \n，作者：未知作者")
        );
    }
    for (status, label) in [
        ("skipped", "已跳过"),
        ("neutral", "中立"),
        ("timed_out", "已超时"),
        ("action_required", "需要处理"),
        ("stale", "已过期"),
        ("queued", "排队中"),
        ("in_progress", "运行中"),
        ("waiting", "等待中"),
        ("pending", "待处理"),
        ("requested", "已请求"),
        ("completed", "已完成"),
    ] {
        assert_eq!(
            deliver_event(
                &app,
                "workflow_run",
                &json!({"workflow_run":{"status":status}})
            ),
            204
        );
        assert_eq!(
            app.messages().last().unwrap()["text"]["content"],
            format!(
                "⚙️ [未知仓库] CI: 工作流 #? {label}\n在 ? 上运行“工作流”（由 ? 触发）-> {label}"
            )
        );
    }
}

#[test]
fn optional_fields_follow_reference_fallback_precedence() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for (event, payload, expected) in [
        (
            "pull_request",
            json!({"repository":{"full_name":"owner/repo"},"number":null,"pull_request":{"number":9,"title":"原始 title","user":null,"html_url":""},"sender":{"login":"sender"}}),
            "[owner/repo] PR #9 已更新: 原始 title\n原始 title，作者：sender",
        ),
        (
            "issues",
            json!({"repository":{"full_name":"owner/repo"},"number":8,"issue":{"number":9,"title":"原始 title","user":{"login":"author"}},"sender":{"login":"ignored"}}),
            "[owner/repo] 议题 #8 已更新: 原始 title\n原始 title，作者：author",
        ),
        (
            "issue_comment",
            json!({"repository":{"full_name":"owner/repo"},"number":8,"issue":{"number":null},"comment":{"body":"原始 content"},"sender":{"login":"sender"}}),
            "[owner/repo] #8 的评论\nsender 评论：\n原始 content",
        ),
        (
            "release",
            json!({"repository":{"full_name":"owner/repo"},"release":{"name":null,"tag_name":"v2","body":null,"html_url":""}}),
            "🚀 [owner/repo] 发布 已更新: v2\nv2\n",
        ),
        (
            "workflow_run",
            json!({"repository":{"full_name":"owner/repo"},"workflow":{"name":"Fallback name"},"workflow_run":{"name":null,"conclusion":null,"status":"in_progress"}}),
            "⚙️ [owner/repo] CI: Fallback name #? 运行中\n在 ? 上运行“Fallback name”（由 ? 触发）-> 运行中",
        ),
        (
            "new_event",
            json!({}),
            "[未知仓库] new_event 事件\n未知触发人 触发了 new_event",
        ),
    ] {
        assert_eq!(deliver_event(&app, event, &payload), 204);
        assert_eq!(app.messages().last().unwrap()["text"]["content"], expected);
    }
    let response = ureq::post(format!("{}/gitea", app.url))
        .header("X-Gitea-Signature", support::signature(b"{}"))
        .send(b"{}".as_slice())
        .unwrap();
    assert_eq!(response.status(), 204);
    assert_eq!(
        app.messages().last().unwrap()["text"]["content"],
        "[未知仓库] 未知 事件\n未知触发人 触发了 未知"
    );
}

fn ntfy_request(app: &Harness, path: &str, body: &[u8], headers: &[(&str, &str)]) -> u16 {
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut request = agent.post(format!("{}{path}", app.url));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.send(body).unwrap().status().as_u16()
}

#[test]
fn ntfy_text_headers_deliver_title_body_link_and_recipients() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/backups",
            "备份完成🦀".as_bytes(),
            &[
                ("Authorization", "Bearer ntfy-secret"),
                ("Title", "Backup"),
                ("Click", "https://example.com/backup")
            ]
        ),
        204
    );
    let messages = app.messages();
    assert_eq!(
        messages[0]["text"]["content"],
        "[backups] Backup\n备份完成🦀\nhttps://example.com/backup"
    );
    assert_eq!(messages[0]["touser"], "roy|jane");
    assert_eq!(messages[0]["toparty"], "2|3");
    assert_eq!(messages[0]["totag"], "4");
}

#[test]
fn ntfy_all_header_aliases_and_named_priorities_map_tags() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for (title, priority, tags, click) in [
        ("X-Title", "X-Priority", "X-Tags", "X-Click"),
        ("Title", "Priority", "Tags", "Click"),
        ("ti", "prio", "tag", "Click"),
        ("t", "p", "ta", "X-Click"),
    ] {
        assert_eq!(
            ntfy_request(
                &app,
                "/ntfy/tool",
                b"body",
                &[
                    ("Authorization", "Bearer ntfy-secret"),
                    (title, "Alert"),
                    (priority, "high"),
                    (tags, "tada, unknown, crab, japan"),
                    (click, "https://example.com")
                ]
            ),
            204
        );
        assert_eq!(
            app.messages().last().unwrap()["text"]["content"],
            "⚠️ 🎉 🦀 🗾 [tool] Alert\nbody\nhttps://example.com"
        );
    }
    for (priority, marker) in [
        ("5", "🚨 "),
        ("max", "🚨 "),
        ("urgent", "🚨 "),
        ("4", "⚠️ "),
        ("3", ""),
        ("default", ""),
        ("2", ""),
        ("low", ""),
        ("1", ""),
        ("min", ""),
    ] {
        assert_eq!(
            ntfy_request(
                &app,
                "/ntfy/tool",
                b"body",
                &[
                    ("Authorization", "Bearer ntfy-secret"),
                    ("Priority", priority)
                ]
            ),
            204
        );
        assert_eq!(
            app.messages().last().unwrap()["text"]["content"],
            format!("{marker}[tool]\nbody")
        );
    }
}

#[test]
fn ntfy_json_ignores_unsupported_fields_and_uses_topic_from_path() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let body = json!({"topic":"ignored", "title":"故障", "message":"中文🦀", "priority":5,
        "tags":["rotating_light", "unknown", "tada"], "click":"https://example.com/status",
        "attach": {"anything":true}, "actions":false, "delay":[1], "markdown":true});
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/tool",
            body.to_string().as_bytes(),
            &[
                ("Authorization", "Bearer ntfy-secret"),
                ("Content-Type", "Application/JSON; charset=utf-8"),
                ("Title", "ignored"),
                ("Priority", "bad")
            ]
        ),
        204
    );
    assert_eq!(
        app.messages()[0]["text"]["content"],
        "🚨 🚨 🎉 [tool] 故障\n中文🦀\nhttps://example.com/status"
    );
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/tool",
            b"{}",
            &[
                ("Authorization", "Bearer ntfy-secret"),
                ("Content-Type", "application/json")
            ]
        ),
        204
    );
    assert_eq!(app.messages()[1]["text"]["content"], "[tool]\n");
}

#[test]
fn ntfy_topic_decodes_utf8_strips_query_and_rejects_invalid_segments() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let auth = [("Authorization", "Bearer ntfy-secret")];
    assert_eq!(
        ntfy_request(&app, "/ntfy/%E5%A4%87%E4%BB%BD?token=private", b"ok", &auth),
        204
    );
    assert_eq!(app.messages()[0]["text"]["content"], "[备份]\nok");
    for path in [
        "/ntfy/",
        "/ntfy/a/b",
        "/ntfy/a/",
        "/ntfy/a%2Fb",
        "/ntfy/a%5Cb",
        "/ntfy/%FF",
        "/ntfy/%",
        "/ntfy/%0A",
        "/ntfy/%20",
        "/ntfy/%2e%2e",
    ] {
        assert_eq!(ntfy_request(&app, path, b"body", &auth), 404, "{path}");
    }
    assert_eq!(app.messages().len(), 1);
}

#[test]
fn ntfy_logs_only_fixed_source_event_and_status() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/private-topic?token=private-query",
            b"private-body",
            &[
                ("Authorization", "Bearer ntfy-secret"),
                ("Title", "private-title"),
                ("X-Gitea-Event", "private-event")
            ]
        ),
        204
    );
    assert_eq!(
        String::from_utf8(app.logs.0.lock().unwrap().clone()).unwrap(),
        "source=ntfy event=publish result=204\n"
    );
}

#[test]
fn ntfy_credentials_fail_closed_and_other_methods_and_root_are_not_publish_apis() {
    let disabled = Harness::start(None, vec![]);
    assert_eq!(
        ntfy_request(
            &disabled,
            "/ntfy/tool",
            b"ok",
            &[("Authorization", "Bearer ntfy-secret")]
        ),
        404
    );
    assert!(disabled.received.lock().unwrap().is_empty());
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for authorization in [
        None,
        Some("Bearer wrong"),
        Some("Basic ntfy-secret"),
        Some("Bearer ntfy-secret-extra"),
        Some("Bearer "),
    ] {
        let headers: Vec<_> = authorization
            .map(|value| ("Authorization", value))
            .into_iter()
            .collect();
        assert_eq!(ntfy_request(&app, "/ntfy/tool", b"{", &headers), 401);
    }
    assert_eq!(
        ntfy_request(&app, "/", b"{}", &[("Authorization", "Bearer ntfy-secret")]),
        404
    );
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    assert_eq!(
        agent
            .put(format!("{}/ntfy/tool", app.url))
            .header("Authorization", "Bearer ntfy-secret")
            .send("ok")
            .unwrap()
            .status(),
        404
    );
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn ntfy_malformed_supported_fields_and_invalid_utf8_return_400_without_delivery() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    for body in [
        "{",
        "null",
        "[]",
        "3",
        r#"{"title":3}"#,
        r#"{"message":null}"#,
        r#"{"click":false}"#,
        r#"{"tags":"warning"}"#,
        r#"{"tags":[3]}"#,
        r#"{"priority":"high"}"#,
        r#"{"priority":0}"#,
        r#"{"priority":6}"#,
        r#"{"priority":-1}"#,
        r#"{"priority":4.5}"#,
    ] {
        assert_eq!(
            ntfy_request(
                &app,
                "/ntfy/tool",
                body.as_bytes(),
                &[
                    ("Authorization", "Bearer ntfy-secret"),
                    ("Content-Type", "application/json")
                ]
            ),
            400,
            "{body}"
        );
    }
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/tool",
            b"\xff",
            &[("Authorization", "Bearer ntfy-secret")]
        ),
        400
    );
    assert_eq!(
        ntfy_request(
            &app,
            "/ntfy/tool",
            b"ok",
            &[("Authorization", "Bearer ntfy-secret"), ("Priority", "bad")]
        ),
        400
    );
    assert!(app.received.lock().unwrap().is_empty());
}

#[test]
fn ntfy_shared_limits_preserve_title_link_and_unicode_and_report_delivery_failure() {
    let app = Harness::start(Some("gitea-secret"), vec![]);
    let headers = [
        ("Authorization", "Bearer ntfy-secret"),
        ("Content-Type", "application/json"),
    ];
    let body =
        json!({"title":"备份🦀", "message":"中文🦀".repeat(1000), "click":"https://example.com"});
    assert_eq!(
        ntfy_request(&app, "/ntfy/tool", body.to_string().as_bytes(), &headers),
        204
    );
    let messages = app.messages();
    let content = messages[0]["text"]["content"].as_str().unwrap();
    assert!(content.starts_with("[tool] 备份🦀\n中文🦀"));
    assert!(content.ends_with("…\nhttps://example.com"));
    assert!(content.len() <= 2048 && content.len() > 2044);
    assert!(!content.contains('\u{fffd}'));
    let exact = json!({"message":"x".repeat(2041)});
    assert_eq!(
        ntfy_request(&app, "/ntfy/tool", exact.to_string().as_bytes(), &headers),
        204
    );
    assert_eq!(
        app.messages()[1]["text"]["content"].as_str().unwrap().len(),
        2048
    );
    for body in [
        json!({"title":"x".repeat(2048)}),
        json!({"click":"x".repeat(2048)}),
        json!({"message":"xxx", "click":"x".repeat(2039)}),
    ] {
        assert_eq!(
            ntfy_request(&app, "/ntfy/tool", body.to_string().as_bytes(), &headers),
            400
        );
    }
    let auth = [
        ("Authorization", "Bearer ntfy-secret"),
        ("Actions", "unsupported"),
        ("Delay", "1h"),
        ("Attach", "https://example.com/a"),
    ];
    assert_eq!(
        ntfy_request(&app, "/ntfy/tool", &vec![b'x'; 1024 * 1024], &auth),
        204
    );
    assert_eq!(
        ntfy_request(&app, "/ntfy/tool", &vec![b'x'; 1024 * 1024 + 1], &auth),
        413
    );
    let failed = Harness::start(Some("gitea-secret"), vec![json!({"errcode":45009})]);
    assert_eq!(ntfy_request(&failed, "/ntfy/tool", b"ok", &auth), 502);
}
