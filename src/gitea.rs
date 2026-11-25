use crate::notification::Notification;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

pub fn signature_valid(body: &[u8], signature: Option<&str>, secret: &str) -> bool {
    let Some(signature) = signature.filter(|s| s.len() == 64) else {
        return false;
    };
    let mut bytes = [0; 32];
    for (index, pair) in signature.as_bytes().chunks_exact(2).enumerate() {
        let Some(high) = (pair[0] as char).to_digit(16) else {
            return false;
        };
        let Some(low) = (pair[1] as char).to_digit(16) else {
            return false;
        };
        bytes[index] = (high * 16 + low) as u8;
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(body);
    mac.verify_slice(&bytes).is_ok()
}

pub fn push(payload: &Value) -> Notification {
    let repo = payload["repository"]["full_name"]
        .as_str()
        .unwrap_or("未知仓库");
    let reference = payload["ref"].as_str().unwrap_or("");
    let branch = reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/tags/"))
        .unwrap_or(reference);
    let commits = payload["commits"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let body = commits
        .iter()
        .take(5)
        .map(|commit| {
            let message = commit["message"]
                .as_str()
                .unwrap_or("")
                .split('\n')
                .next()
                .unwrap_or("")
                .trim();
            let author = commit["author"]["name"]
                .as_str()
                .or_else(|| commit["author"]["username"].as_str())
                .unwrap_or("未知作者");
            format!("- {message} ({author})")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Notification {
        title: format!("[{repo}] {} 次提交 -> {branch}", commits.len()),
        body,
        link: payload["compare_url"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    }
}
