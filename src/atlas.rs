use crate::notification::Notification;
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;

pub fn signature_valid(body: &[u8], signature: Option<&str>, secret: &str) -> bool {
    let Some(signature) = signature.filter(|signature| signature.len() == 28) else {
        return false;
    };
    let Ok(signature) = STANDARD.decode(signature) else {
        return false;
    };
    let mut mac =
        Hmac::<Sha1>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(body);
    mac.verify_slice(&signature).is_ok()
}

pub fn parse(body: &[u8]) -> Option<Notification> {
    let payload: Value = serde_json::from_slice(body).ok()?;
    if !payload.is_object()
        || [
            "status",
            "eventTypeName",
            "clusterName",
            "humanReadable",
            "groupId",
        ]
        .iter()
        .any(|key| !payload[key].is_null() && !payload[key].is_string())
    {
        return None;
    }
    Some(Notification {
        title: format!(
            "[Atlas {}] {}（{}）",
            payload["status"].as_str().unwrap_or("未知状态"),
            payload["eventTypeName"].as_str().unwrap_or("未知事件"),
            payload["clusterName"].as_str().unwrap_or("未知集群")
        ),
        body: payload["humanReadable"]
            .as_str()
            .unwrap_or("无告警描述")
            .into(),
        link: payload["groupId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(|id| format!("https://cloud.mongodb.com/v2/{id}#/alerts")),
    })
}
