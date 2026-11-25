mod atlas;
mod config;
mod gitea;
mod notification;
mod ntfy;
mod wecom;
pub use config::Config;
use std::{
    io::{self, Read},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tiny_http::{Method, Request, Response, Server};

pub struct Service {
    server: Server,
    config: Config,
    wecom: wecom::Client,
}

impl Service {
    pub fn new(config: Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let server = Server::http(config.listen_addr)?;
        Ok(Self {
            server,
            config,
            wecom: wecom::Client::new(),
        })
    }
    pub fn addr(&self) -> SocketAddr {
        self.server.server_addr().to_ip().expect("IP listener")
    }
    pub fn run(mut self, stop: Arc<AtomicBool>, mut log: impl io::Write) -> io::Result<()> {
        while !stop.load(Ordering::Relaxed) {
            if let Some(mut request) = self.server.recv_timeout(Duration::from_millis(25))? {
                let source = match request.url() {
                    "/gitea" => "gitea",
                    "/atlas" => "atlas",
                    "/health" => "health",
                    path if ntfy::topic(path).is_some() => "ntfy",
                    _ => "unknown",
                };
                let event_header = if source == "atlas" {
                    "X-MMS-Event"
                } else {
                    "X-Gitea-Event"
                };
                let event = match (source, header(&request, event_header)) {
                    (
                        "gitea",
                        Some(
                            event @ ("push" | "pull_request" | "issues" | "issue_comment"
                            | "release" | "workflow_run"),
                        ),
                    ) => event.to_owned(),
                    (
                        "atlas",
                        Some(
                            event @ ("alert.open" | "alert.close" | "alert.update"
                            | "alert.acknowledge" | "alert.cancel" | "alert.inform"),
                        ),
                    ) => event.to_owned(),
                    ("ntfy", _) => "publish".into(),
                    _ => "unknown".into(),
                };
                let status = self.handle(&mut request);
                writeln!(log, "source={source} event={event} result={status}")?;
                let _ = request.respond(Response::empty(status));
            }
        }
        Ok(())
    }
    fn handle(&mut self, request: &mut Request) -> u16 {
        const MAX_BODY: u64 = 1024 * 1024;
        if request
            .body_length()
            .is_some_and(|length| length > MAX_BODY as usize)
        {
            return 413;
        }
        let mut body = Vec::new();
        if request
            .as_reader()
            .take(MAX_BODY + 1)
            .read_to_end(&mut body)
            .is_err()
        {
            return 400;
        }
        if body.len() > MAX_BODY as usize {
            return 413;
        }
        match (request.method(), request.url()) {
            (&Method::Get, "/health") => 200,
            (&Method::Post, "/atlas") => {
                let Some(secret) = &self.config.atlas_secret else {
                    return 404;
                };
                if !atlas::signature_valid(&body, header(request, "X-MMS-Signature"), secret) {
                    return 401;
                }
                let Some(notification) = atlas::parse(&body) else {
                    return 400;
                };
                let Some(content) = notification.render() else {
                    return 400;
                };
                if self.wecom.deliver(&self.config, &content).is_ok() {
                    204
                } else {
                    502
                }
            }
            (&Method::Post, "/gitea") => {
                let Some(secret) = &self.config.gitea_secret else {
                    return 404;
                };
                if !gitea::signature_valid(&body, header(request, "X-Gitea-Signature"), secret) {
                    return 401;
                }
                let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
                    return 400;
                };
                if !payload.is_object() {
                    return 400;
                }
                let notification = match header(request, "X-Gitea-Event") {
                    Some("push") => gitea::push(&payload),
                    Some("pull_request") => gitea::pull_request(&payload),
                    Some("issues") => gitea::issue(&payload),
                    Some("issue_comment") => gitea::issue_comment(&payload),
                    Some("release") => gitea::release(&payload),
                    Some("workflow_run") => gitea::workflow_run(&payload),
                    event => gitea::fallback(event, &payload),
                };
                let Some(content) = notification.render() else {
                    return 400;
                };
                if self.wecom.deliver(&self.config, &content).is_ok() {
                    204
                } else {
                    502
                }
            }
            (&Method::Post, path) if ntfy::topic(path).is_some() => {
                let Some(token) = &self.config.ntfy_token else {
                    return 404;
                };
                if !ntfy::authorized(header(request, "Authorization"), token) {
                    return 401;
                }
                let topic = ntfy::topic(path).expect("validated topic");
                let Some(notification) = ntfy::parse(request, &body, &topic) else {
                    return 400;
                };
                let Some(content) = notification.render() else {
                    return 400;
                };
                if self.wecom.deliver(&self.config, &content).is_ok() {
                    204
                } else {
                    502
                }
            }
            (_, "/health" | "/gitea" | "/atlas") => 405,
            _ => 404,
        }
    }
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}
