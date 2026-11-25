use crate::{header, notification::Notification};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tiny_http::Request;

pub fn topic(path: &str) -> Option<String> {
    let topic = path.split('?').next()?.strip_prefix("/ntfy/")?;
    let mut decoded = Vec::new();
    let mut bytes = topic.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let topic = String::from_utf8(decoded).ok()?;
    if topic.is_empty()
        || matches!(topic.as_str(), "." | "..")
        || topic
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
    {
        return None;
    }
    Some(topic)
}

pub fn authorized(authorization: Option<&str>, token: &str) -> bool {
    let Some(candidate) = authorization.and_then(|s| s.strip_prefix("Bearer ")) else {
        return false;
    };
    let mut expected =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepts any key size");
    expected.update(token.as_bytes());
    let mut supplied =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepts any key size");
    supplied.update(candidate.as_bytes());
    supplied
        .verify_slice(&expected.finalize().into_bytes())
        .is_ok()
}

fn alias<'a>(request: &'a Request, names: &[&'static str]) -> Option<&'a str> {
    names.iter().find_map(|name| header(request, name))
}

fn priority(value: &str) -> Option<u8> {
    match value {
        "1" | "min" => Some(1),
        "2" | "low" => Some(2),
        "3" | "default" => Some(3),
        "4" | "high" => Some(4),
        "5" | "max" | "urgent" => Some(5),
        _ => None,
    }
}

pub fn parse(request: &Request, body: &[u8], topic: &str) -> Option<Notification> {
    if header(request, "Content-Type").is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
    }) {
        let payload: serde_json::Value = serde_json::from_slice(body).ok()?;
        let object = payload.as_object()?;
        let string = |name| match object.get(name) {
            None => Some(""),
            Some(value) => value.as_str(),
        };
        let priority = match object.get("priority") {
            None => 3,
            Some(value) => u8::try_from(value.as_u64()?)
                .ok()
                .filter(|n| (1..=5).contains(n))?,
        };
        let tags = match object.get("tags") {
            None => Vec::new(),
            Some(value) => value
                .as_array()?
                .iter()
                .map(|v| v.as_str())
                .collect::<Option<Vec<_>>>()?,
        };
        return Some(notification(
            topic,
            string("title")?,
            string("message")?,
            priority,
            tags.into_iter(),
            Some(string("click")?),
        ));
    }
    let title = alias(request, &["X-Title", "Title", "ti", "t"]).unwrap_or("");
    let priority = alias(request, &["X-Priority", "Priority", "prio", "p"])
        .map(priority)
        .unwrap_or(Some(3))?;
    let tags = alias(request, &["X-Tags", "Tags", "tag", "ta"]).unwrap_or("");
    Some(notification(
        topic,
        title,
        std::str::from_utf8(body).ok()?,
        priority,
        tags.split(',').map(str::trim),
        alias(request, &["X-Click", "Click"]),
    ))
}

fn notification<'a>(
    topic: &str,
    title: &str,
    body: &str,
    priority: u8,
    tags: impl Iterator<Item = &'a str>,
    click: Option<&str>,
) -> Notification {
    static EMOJIS: std::sync::OnceLock<std::collections::HashMap<String, String>> =
        std::sync::OnceLock::new();
    let emojis = EMOJIS.get_or_init(|| {
        serde_json::from_str(include_str!("data/ntfy-emojis.json"))
            .expect("vendored ntfy emoji map")
    });
    let mut prefix = match priority {
        4 => "⚠️ ".into(),
        5 => "🚨 ".into(),
        _ => String::new(),
    };
    for tag in tags {
        if let Some(emoji) = emojis.get(tag) {
            prefix.push_str(emoji);
            prefix.push(' ');
        }
    }
    Notification {
        title: if title.is_empty() {
            format!("{prefix}[{topic}]")
        } else {
            format!("{prefix}[{topic}] {title}")
        },
        body: body.into(),
        link: click.filter(|s| !s.is_empty()).map(str::to_owned),
    }
}
