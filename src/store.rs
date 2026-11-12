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

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub usage: BTreeMap<String, u32>,
    pub favorites: BTreeSet<String>,
    pub disabled: BTreeSet<String>,
    pub developers: BTreeSet<PathBuf>,
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
