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
    /// Built-in extensions are opt-in, so they are tracked separately from
    /// `disabled`, which lists the installed extensions a user turned off.
    pub enabled_builtins: BTreeSet<String>,
    /// Extension preferences the user set, keyed by `ext` for the extension's
    /// own and `ext/cmd` for a command's. Unset keys fall back to the manifest.
    pub preferences: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
}
impl Store {
    pub fn is_enabled(&self, ext: &crate::extensions::package::Extension) -> bool {
        if ext.builtin {
            self.enabled_builtins.contains(&ext.manifest.id)
        } else {
            !self.disabled.contains(&ext.manifest.id)
        }
    }
    /// The extension's and the command's preferences merged, with defaults applied.
    pub fn preference_values(
        &self,
        ext: &crate::extensions::package::Extension,
        command: &str,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut values = serde_json::Map::new();
        let id = &ext.manifest.id;
        let scopes = std::iter::once((id.clone(), &ext.manifest.preferences)).chain(
            ext.manifest
                .commands
                .iter()
                .filter(|c| c.id == command)
                .map(|c| (format!("{id}/{}", c.id), &c.preferences)),
        );
        for (key, preferences) in scopes {
            let set = self.preferences.get(&key);
            for p in preferences {
                let value = set.and_then(|m| m.get(&p.name)).cloned().unwrap_or_else(|| p.fallback());
                values.insert(p.name.clone(), value);
            }
        }
        values
    }
    /// The title of a required preference that is still empty, if any.
    pub fn missing_preference(
        &self,
        ext: &crate::extensions::package::Extension,
        command: &str,
    ) -> Option<String> {
        let values = self.preference_values(ext, command);
        let command = ext.manifest.commands.iter().find(|c| c.id == command);
        ext.manifest
            .preferences
            .iter()
            .chain(command.into_iter().flat_map(|c| &c.preferences))
            .find(|p| p.required && values.get(&p.name).is_none_or(|v| v.as_str() == Some("")))
            .map(|p| p.title.clone())
    }
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
    #[test]
    fn builtins_are_opt_in_and_installed_extensions_opt_out() {
        let old: Store = serde_json::from_str(r#"{"disabled":["other"]}"#).unwrap();
        assert!(old.enabled_builtins.is_empty());
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links");
        let mut ext = crate::extensions::package::load(&path).unwrap();
        let mut store = Store::default();
        assert!(store.is_enabled(&ext));
        store.disabled.insert(ext.manifest.id.clone());
        assert!(!store.is_enabled(&ext));
        ext.builtin = true;
        assert!(!store.is_enabled(&ext));
        store.enabled_builtins.insert(ext.manifest.id.clone());
        assert!(store.is_enabled(&ext));
    }
    #[test]
    fn preferences_merge_defaults_and_overrides() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("index.mjs"), "").unwrap();
        fs::write(temp.path().join("extension.json"), r#"{"id":"test.prefs","name":"P","version":"1","sdk_version":1,"entry":"index.mjs",
            "preferences":[{"name":"units","type":"dropdown","title":"Units","data":[{"title":"Metric","value":"metric"},{"title":"Imperial","value":"imperial"}]},
                           {"name":"key","type":"password","title":"API key","required":true}],
            "commands":[{"id":"a","title":"A","preferences":[{"name":"fast","type":"checkbox","title":"Fast","default":true}]},{"id":"b","title":"B"}]}"#).unwrap();
        let ext = crate::extensions::package::load(temp.path()).unwrap();
        let mut store = Store::default();
        let values = store.preference_values(&ext, "a");
        assert_eq!(values["units"], "metric");
        assert_eq!(values["fast"], true);
        assert!(!store.preference_values(&ext, "b").contains_key("fast"));
        assert_eq!(store.missing_preference(&ext, "a").as_deref(), Some("API key"));
        store.preferences.entry("test.prefs".into()).or_default().insert("key".into(), "s3cret".into());
        store.preferences.entry("test.prefs/a".into()).or_default().insert("fast".into(), false.into());
        assert_eq!(store.missing_preference(&ext, "a"), None);
        assert_eq!(store.preference_values(&ext, "a")["fast"], false);
    }
}
