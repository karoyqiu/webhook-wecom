use crate::notification::Notification;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

pub fn signature_valid(body: &[u8], signature: Option<&str>, secret: &str) -> bool {
    let Some(signature) = signature.filter(|s| s.len() == 64) else {
        return false;
    };
    let mut bytes = [0; 32];
    for (index, pair) in signature.as_bytes().as_chunks::<2>().0.iter().enumerate() {
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

fn repo(payload: &Value) -> &str {
    payload["repository"]["full_name"]
        .as_str()
        .unwrap_or("未知仓库")
}

fn action(payload: &Value) -> &str {
    match payload["action"].as_str().unwrap_or("updated") {
        "opened" => "已创建",
        "closed" => "已关闭",
        "reopened" => "已重新打开",
        "updated" => "已更新",
        "published" => "已发布",
        "edited" => "已编辑",
        "assigned" => "已指派",
        "unassigned" => "已取消指派",
        "label_updated" => "标签已更新",
        "label_cleared" => "标签已清除",
        "synchronized" => "已同步",
        "created" => "已创建",
        "deleted" => "已删除",
        "prereleased" => "已预发布",
        "released" => "已正式发布",
        "reviewed" => "已评审",
        "review_requested" => "已请求评审",
        "review_request_removed" => "已取消评审请求",
        "ready_for_review" => "已准备评审",
        "converted_to_draft" => "已转为草稿",
        other => other,
    }
}

fn number(value: &Value) -> String {
    value
        .as_u64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".into())
}

fn link(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn truncated(value: &Value) -> String {
    let mut chars = value.as_str().unwrap_or("").chars();
    let mut text: String = chars.by_ref().take(300).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    text
}

pub fn issue_comment(payload: &Value) -> Notification {
    let comment = &payload["comment"];
    let id = number(
        payload["issue"]
            .get("number")
            .filter(|v| !v.is_null())
            .unwrap_or(&payload["number"]),
    );
    let author = comment["user"]["login"]
        .as_str()
        .or_else(|| payload["sender"]["login"].as_str())
        .unwrap_or("未知评论者");
    Notification {
        title: format!("[{}] #{id} 的评论", repo(payload)),
        body: format!("{author} 评论：\n{}", truncated(&comment["body"])),
        link: link(&comment["html_url"]),
    }
}

pub fn release(payload: &Value) -> Notification {
    let release = &payload["release"];
    let tag = release["tag_name"].as_str().unwrap_or("");
    let name = release["name"].as_str().unwrap_or(tag);
    Notification {
        title: format!("🚀 [{}] 发布 {}: {tag}", repo(payload), action(payload)),
        body: format!("{name}\n{}", truncated(&release["body"])),
        link: link(&release["html_url"]),
    }
}

pub fn workflow_run(payload: &Value) -> Notification {
    let run = &payload["workflow_run"];
    let name = run["name"]
        .as_str()
        .or_else(|| payload["workflow"]["name"].as_str())
        .unwrap_or("工作流");
    let result = run["conclusion"]
        .as_str()
        .or_else(|| run["status"].as_str())
        .unwrap_or("unknown");
    let marker = match result {
        "success" => "✅",
        "failure" => "❌",
        _ => "⚙️",
    };
    let result = match result {
        "success" => "成功",
        "failure" => "失败",
        "cancelled" => "已取消",
        "skipped" => "已跳过",
        "neutral" => "中立",
        "timed_out" => "已超时",
        "action_required" => "需要处理",
        "stale" => "已过期",
        "queued" => "排队中",
        "in_progress" => "运行中",
        "waiting" => "等待中",
        "pending" => "待处理",
        "requested" => "已请求",
        "completed" => "已完成",
        "unknown" => "未知",
        other => other,
    };
    let id = number(&run["run_number"]);
    let branch = run["head_branch"].as_str().unwrap_or("?");
    let event = run["event"].as_str().unwrap_or("?");
    Notification {
        title: format!("{marker} [{}] CI: {name} #{id} {result}", repo(payload)),
        body: format!("在 {branch} 上运行“{name}”（由 {event} 触发）-> {result}"),
        link: link(&run["html_url"]),
    }
}

pub fn fallback(event: Option<&str>, payload: &Value) -> Notification {
    let event = event.unwrap_or("未知");
    let actor = payload["sender"]["login"]
        .as_str()
        .or_else(|| payload["pusher"]["login"].as_str())
        .unwrap_or("未知触发人");
    Notification {
        title: format!("[{}] {event} 事件", repo(payload)),
        body: format!("{actor} 触发了 {event}"),
        link: None,
    }
}

pub fn pull_request(payload: &Value) -> Notification {
    let pr = &payload["pull_request"];
    let id = number(
        payload
            .get("number")
            .filter(|v| !v.is_null())
            .unwrap_or(&pr["number"]),
    );
    let title = pr["title"].as_str().unwrap_or("");
    let author = pr["user"]["login"]
        .as_str()
        .or_else(|| payload["sender"]["login"].as_str())
        .unwrap_or("未知作者");
    let marker = if payload["action"] == "closed" {
        "🔀 "
    } else {
        ""
    };
    Notification {
        title: format!(
            "{marker}[{}] PR #{id} {}: {title}",
            repo(payload),
            action(payload)
        ),
        body: format!("{title}，作者：{author}"),
        link: link(&pr["html_url"]),
    }
}

pub fn issue(payload: &Value) -> Notification {
    let issue = &payload["issue"];
    let id = number(
        payload
            .get("number")
            .filter(|v| !v.is_null())
            .unwrap_or(&issue["number"]),
    );
    let title = issue["title"].as_str().unwrap_or("");
    let author = issue["user"]["login"]
        .as_str()
        .or_else(|| payload["sender"]["login"].as_str())
        .unwrap_or("未知作者");
    Notification {
        title: format!(
            "[{}] 议题 #{id} {}: {title}",
            repo(payload),
            action(payload)
        ),
        body: format!("{title}，作者：{author}"),
        link: link(&issue["html_url"]),
    }
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
