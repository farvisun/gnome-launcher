use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default)]
    pub keywords: String,
    pub kind: Kind,
}
fn default_icon() -> String {
    "application-x-executable-symbolic".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    App(String),
    File(String),
    Calculation(String),
    Command { extension: String, command: String },
    Settings,
}

#[cfg(test)]
pub fn relevance(query: &str, title: &str, keywords: &str) -> Option<u32> {
    score(
        &query.to_lowercase(),
        &title.to_lowercase(),
        &keywords.to_lowercase(),
    )
}

/// Ranks pre-lowercased text. The palette lowercases its catalog once so a
/// keystroke only pays for comparisons, not allocations per candidate.
pub fn score(q: &str, t: &str, keywords: &str) -> Option<u32> {
    if q.is_empty() {
        return Some(0);
    }
    if t == q {
        return Some(4000);
    }
    if t.starts_with(q) {
        return Some(3000);
    }
    if t.split_whitespace().any(|w| w.starts_with(q)) {
        return Some(2500);
    }
    if t.contains(q) {
        return Some(2000);
    }
    let mut chars = q.chars();
    let mut next = chars.next();
    let mut gaps = 0;
    for ch in t.chars() {
        if next == Some(ch) {
            next = chars.next();
        } else {
            gaps += 1;
        }
        if next.is_none() {
            return Some(1000_u32.saturating_sub(gaps));
        }
    }
    keywords.contains(q).then_some(500)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranks_matches_and_unicode() {
        assert!(
            relevance("files", "Files", "").unwrap()
                > relevance("files", "Files and folders", "").unwrap()
        );
        assert!(relevance("vsc", "Visual Studio Code", "").is_some());
        assert!(relevance("東京", "東京 Photos", "").is_some());
        assert_eq!(relevance("zzzz", "Files", ""), None);
    }
}
