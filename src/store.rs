use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| gtk::glib::home_dir().join(".local/share"))
        .join("gnome-launcher")
}

/// How search results are grouped when several providers match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResultOrder {
    /// Applications always lead; other sections follow by best match.
    #[default]
    AppsFirst,
    /// Every section is ordered by its best match.
    Relevance,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub usage: BTreeMap<String, u32>,
    pub favorites: BTreeSet<String>,
    pub disabled: BTreeSet<String>,
    pub developers: BTreeSet<PathBuf>,
    pub result_order: ResultOrder,
}
impl Store {
    pub fn load() -> Self {
        Self::load_at(&data_dir().join("state.json"))
    }
    pub fn load_at(path: &Path) -> Self {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                let backup = path.with_extension(format!(
                    "corrupt-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                match fs::copy(path, &backup) {
                    Ok(_) => eprintln!(
                        "Cannot read {}: {e}; saved original at {}",
                        path.display(),
                        backup.display()
                    ),
                    Err(backup_error) => {
                        eprintln!("Cannot back up malformed preferences: {backup_error}")
                    }
                }
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
    pub fn save(&self) -> Result<()> {
        write_json(&data_dir().join("state.json"), self)
    }
    pub fn used(&mut self, id: &str) {
        let count = self.usage.entry(id.to_owned()).or_default();
        *count = count.saturating_add(1);
    }
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let parent = path.parent().context("Missing parent directory")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod order_tests {
    use super::*;
    #[test]
    fn result_order_defaults_to_apps_first_and_round_trips() {
        let old: Store = serde_json::from_str(r#"{"usage":{}}"#).unwrap();
        assert_eq!(old.result_order, ResultOrder::AppsFirst);
        let store = Store { result_order: ResultOrder::Relevance, ..Default::default() };
        let text = serde_json::to_string(&store).unwrap();
        assert!(text.contains(r#""result_order":"relevance""#));
        let back: Store = serde_json::from_str(&text).unwrap();
        assert_eq!(back.result_order, ResultOrder::Relevance);
    }
}
