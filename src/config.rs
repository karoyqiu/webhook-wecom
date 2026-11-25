use std::{env, net::SocketAddr};

pub struct Config {
    pub listen_addr: SocketAddr,
    pub wecom_api_base: String,
    pub corp_id: String,
    pub agent_id: u64,
    pub secret: String,
    pub to_user: String,
    pub to_party: String,
    pub to_tag: String,
    pub gitea_secret: Option<String>,
    pub atlas_secret: Option<String>,
    pub ntfy_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let required = |name: &str| optional(name).ok_or_else(|| format!("{name} is required"));
        let corp_id = required("WECOM_CORP_ID")?;
        let agent_id = required("WECOM_AGENT_ID")?
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or("WECOM_AGENT_ID must be a positive integer")?;
        let secret = required("WECOM_SECRET")?;
        let to_user = optional("WECOM_TO_USER").unwrap_or_default();
        let to_party = optional("WECOM_TO_PARTY").unwrap_or_default();
        let to_tag = optional("WECOM_TO_TAG").unwrap_or_default();
        if to_user.is_empty() && to_party.is_empty() && to_tag.is_empty() {
            return Err("WECOM_TO_USER, WECOM_TO_PARTY or WECOM_TO_TAG is required".into());
        }
        let listen_addr = optional("LISTEN_ADDR")
            .unwrap_or_else(|| "127.0.0.1:8080".into())
            .parse()
            .map_err(|_| "LISTEN_ADDR must be an IP address and port")?;
        let wecom_api_base =
            optional("WECOM_API_BASE").unwrap_or_else(|| "https://qyapi.weixin.qq.com".into());
        let uri: ureq::http::Uri = wecom_api_base
            .parse()
            .map_err(|_| "WECOM_API_BASE must be an HTTP(S) origin")?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().is_none()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
        {
            return Err("WECOM_API_BASE must be an HTTP(S) origin".into());
        }
        Ok(Self {
            listen_addr,
            wecom_api_base: wecom_api_base.trim_end_matches('/').into(),
            corp_id,
            agent_id,
            secret,
            to_user,
            to_party,
            to_tag,
            gitea_secret: optional("GITEA_WEBHOOK_SECRET"),
            atlas_secret: optional("ATLAS_WEBHOOK_SECRET"),
            ntfy_token: optional("NTFY_TOKEN"),
        })
    }
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}
