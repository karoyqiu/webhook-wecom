mod support;
use serde_json::json;
use support::{Harness, push_fixture};

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
