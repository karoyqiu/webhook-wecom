use std::process::Command;

fn configured_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_webhook-wecom"));
    command
        .env_clear()
        .env("WECOM_CORP_ID", "corp-private")
        .env("WECOM_AGENT_ID", "1000002")
        .env("WECOM_SECRET", "secret-private")
        .env("WECOM_TO_USER", "roy");
    // Windows sockets need SystemRoot even when application configuration is isolated.
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
}

#[test]
fn blank_required_values_and_invalid_configuration_fail_with_variable_name() {
    for (key, value) in [
        ("WECOM_CORP_ID", " "),
        ("WECOM_SECRET", ""),
        ("WECOM_TO_USER", " "),
        ("WECOM_AGENT_ID", "0"),
        ("WECOM_AGENT_ID", "secret-private"),
        ("LISTEN_ADDR", "secret-private"),
        ("WECOM_API_BASE", "https://user:secret-private@example.com"),
        ("WECOM_API_BASE", "https://example.com/path?secret-private"),
        ("WECOM_API_BASE", "ftp://example.com"),
    ] {
        let result = configured_command().env(key, value).output().unwrap();
        assert!(!result.status.success(), "{key} must fail");
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(error.contains(key), "{error}");
        assert!(!error.contains("secret-private"), "{error}");
    }
}

struct Running(std::process::Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn environment_listener_and_each_recipient_kind_work_and_blank_source_secret_disables_endpoint() {
    use std::{
        net::TcpListener,
        process::Stdio,
        time::{Duration, Instant},
    };
    for recipient in ["WECOM_TO_USER", "WECOM_TO_PARTY", "WECOM_TO_TAG"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let mut process = Running(
            configured_command()
                .env_remove("WECOM_TO_USER")
                .env(recipient, "1")
                .env("LISTEN_ADDR", addr.to_string())
                .env("GITEA_WEBHOOK_SECRET", " ")
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(200)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(response) = agent.get(format!("http://{addr}/health")).call() {
                assert_eq!(response.status(), 200);
                break;
            }
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "service exited before health check"
            );
            assert!(Instant::now() < deadline, "health check timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            agent
                .post(format!("http://{addr}/gitea"))
                .send("{}")
                .unwrap()
                .status(),
            404
        );
    }
}

#[test]
fn missing_or_invalid_environment_fails_without_disclosing_values() {
    let required = [
        "WECOM_CORP_ID",
        "WECOM_AGENT_ID",
        "WECOM_SECRET",
        "WECOM_TO_USER",
    ];
    for missing in required {
        let mut command = Command::new(env!("CARGO_BIN_EXE_webhook-wecom"));
        command.env_clear();
        for key in required {
            if key != missing {
                command.env(
                    key,
                    if key == "WECOM_AGENT_ID" {
                        "1000002"
                    } else {
                        "private-value"
                    },
                );
            }
        }
        let result = command.output().unwrap();
        assert!(!result.status.success(), "missing {missing} must fail");
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(error.contains(missing), "{error}");
        assert!(!error.contains("private-value"));
    }
}
