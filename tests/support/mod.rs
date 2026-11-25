use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use webhook_wecom::{Config, Service};

#[derive(Clone, Default)]
pub struct Logs(pub Arc<Mutex<Vec<u8>>>);
impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Harness {
    pub url: String,
    pub received: Arc<Mutex<Vec<(String, Value)>>>,
    pub logs: Logs,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl Harness {
    pub fn start(secret: Option<&str>, replies: Vec<Value>) -> Self {
        let mut replies: VecDeque<_> = replies.into();
        let mut tokens = 0;
        Self::start_with(secret, move |path| {
            let reply = if path.starts_with("/cgi-bin/gettoken?") {
                tokens += 1;
                json!({"errcode":0,"access_token":format!("access-private-{tokens}"),"expires_in":7200})
            } else {
                replies.pop_front().unwrap_or(json!({"errcode":0}))
            };
            (200, reply.to_string())
        })
    }

    pub fn start_with(
        secret: Option<&str>,
        mut reply: impl FnMut(&str) -> (u16, String) + Send + 'static,
    ) -> Self {
        let fake = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", fake.server_addr());
        let config = Config {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            wecom_api_base: base,
            corp_id: "corp &?private".into(),
            agent_id: 1000002,
            secret: "wecom-secret&?".into(),
            to_user: "roy|jane".into(),
            to_party: "2|3".into(),
            to_tag: "4".into(),
            gitea_secret: secret.map(str::to_owned),
            atlas_secret: secret.map(|_| "atlas-secret".into()),
            ntfy_token: secret.map(|_| "ntfy-secret".into()),
        };
        let service = Service::new(config).unwrap();
        let url = format!("http://{}", service.addr());
        let stop = Arc::new(AtomicBool::new(false));
        let received = Arc::new(Mutex::new(Vec::new()));
        let records = received.clone();
        let stopping = stop.clone();
        let fake_worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                if let Some(mut request) = fake.recv_timeout(Duration::from_millis(25)).unwrap() {
                    let path = request.url().to_owned();
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).unwrap();
                    records.lock().unwrap().push((
                        path.clone(),
                        serde_json::from_str(&body).unwrap_or(Value::Null),
                    ));
                    let (status, body) = reply(&path);
                    request
                        .respond(tiny_http::Response::from_string(body).with_status_code(status))
                        .unwrap();
                }
            }
        });
        let stopping = stop.clone();
        let logs = Logs::default();
        let output = logs.clone();
        let worker = thread::spawn(move || service.run(stopping, output).unwrap());
        Self {
            url,
            received,
            logs,
            stop,
            workers: vec![worker, fake_worker],
        }
    }

    pub fn request(&self, path: &str, body: &[u8], signature: Option<&str>, event: &str) -> u16 {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut request = agent
            .post(format!("{}{path}", self.url))
            .header("X-Gitea-Event", event);
        if let Some(signature) = signature {
            request = request.header("X-Gitea-Signature", signature);
        }
        request.send(body).unwrap().status().as_u16()
    }

    pub fn push(&self, payload: &Value) -> u16 {
        let body = payload.to_string();
        self.request(
            "/gitea",
            body.as_bytes(),
            Some(&signature(body.as_bytes())),
            "push",
        )
    }

    pub fn messages(&self) -> Vec<Value> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.starts_with("/cgi-bin/message/send?"))
            .map(|(_, body)| body.clone())
            .collect()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut failed = false;
        for worker in self.workers.drain(..) {
            failed |= worker.join().is_err();
        }
        assert!(!failed || thread::panicking(), "HTTP worker panicked");
    }
}

pub fn signature(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(b"gitea-secret").unwrap();
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn push_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/push.json")).unwrap()
}
