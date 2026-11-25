pub(crate) struct Notification {
    pub title: String,
    pub body: String,
    pub link: Option<String>,
}
impl Notification {
    pub fn render(&self) -> Option<String> {
        let suffix = self
            .link
            .as_ref()
            .map(|link| format!("\n{link}"))
            .unwrap_or_default();
        let budget = 2048usize.checked_sub(self.title.len() + 1 + suffix.len())?;
        let body = if self.body.len() <= budget {
            self.body.clone()
        } else {
            let mut end = budget.checked_sub("…".len())?;
            while !self.body.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &self.body[..end])
        };
        Some(format!("{}\n{body}{suffix}", self.title))
    }
}
