use crate::Config;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
pub(crate) struct Client {
    agent: ureq::Agent,
    token: Option<(String, Instant)>,
}
impl Client {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .max_redirects(0)
                .build()
                .new_agent(),
            token: None,
        }
    }
    pub fn deliver(&mut self, config: &Config, content: &str) -> Result<(), ()> {
        if self
            .token
            .as_ref()
            .is_none_or(|(_, expiry)| Instant::now() >= *expiry)
        {
            self.refresh(config)?;
        }
        for attempt in 0..2 {
            let code = self.send(config, content)?;
            if code == 0 {
                return Ok(());
            }
            if matches!(code, 40014 | 42001) {
                self.token = None;
                if attempt == 0 {
                    self.refresh(config)?;
                    continue;
                }
            }
            return Err(());
        }
        Err(())
    }
    fn send(&self, config: &Config, content: &str) -> Result<i64, ()> {
        let token = &self.token.as_ref().ok_or(())?.0;
        let mut response = self.agent.post(format!("{}/cgi-bin/message/send", config.wecom_api_base))
            .query("access_token", token)
            .send_json(json!({"touser":config.to_user,"toparty":config.to_party,"totag":config.to_tag,"agentid":config.agent_id,"msgtype":"text","text":{"content":content}}))
            .map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        let result: Value = response.body_mut().read_json().map_err(|_| ())?;
        result["errcode"].as_i64().ok_or(())
    }
    fn refresh(&mut self, config: &Config) -> Result<(), ()> {
        self.token = None;
        let mut response = self
            .agent
            .get(format!("{}/cgi-bin/gettoken", config.wecom_api_base))
            .query("corpid", &config.corp_id)
            .query("corpsecret", &config.secret)
            .call()
            .map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        let result: Value = response.body_mut().read_json().map_err(|_| ())?;
        if result["errcode"].as_i64() != Some(0) {
            return Err(());
        }
        let token = result["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(())?;
        let seconds = result["expires_in"].as_u64().filter(|s| *s > 0).ok_or(())?;
        let expiry = Instant::now()
            .checked_add(Duration::from_secs(seconds))
            .ok_or(())?;
        self.token = Some((token.into(), expiry));
        Ok(())
    }
}
