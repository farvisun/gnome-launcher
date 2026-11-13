use crate::store;
use anyhow::{Context, Result, bail, ensure};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const MAX_PACKAGE: u64 = 20 * 1024 * 1024;
pub const MAX_UNPACKED: u64 = 50 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sdk_version: u32,
    pub entry: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub commands: Vec<Command>,
    /// Settings shared by every command, shown in Lumen's Settings.
    #[serde(default)]
    pub preferences: Vec<Preference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// A GNOME icon name shown next to the command in search results.
    #[serde(default)]
    pub icon: String,
    /// `view` (default) opens a view; `no-view` runs, then closes Lumen.
    #[serde(default)]
    pub mode: Mode,
    /// Values typed beside the command in the root search, like Raycast arguments.
    #[serde(default)]
    pub arguments: Vec<Argument>,
    #[serde(default)]
    pub preferences: Vec<Preference>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    View,
    NoView,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub title: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub name: String,
    #[serde(default)]
    pub placeholder: String,
    /// `text`, `password` or `dropdown`.
    #[serde(default = "text_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub data: Vec<Choice>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preference {
    pub name: String,
    /// `textfield`, `password`, `checkbox` or `dropdown`.
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// The checkbox's own label.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub data: Vec<Choice>,
}
fn text_type() -> String {
    "text".into()
}
impl Preference {
    /// The value used until the user sets one.
    pub fn fallback(&self) -> serde_json::Value {
        match (&self.default, self.kind.as_str()) {
            (Some(value), _) => value.clone(),
            (None, "checkbox") => false.into(),
            (None, "dropdown") => self.data.first().map(|c| c.value.clone()).unwrap_or_default().into(),
            _ => "".into(),
        }
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn check_choices(data: &[Choice], dropdown: bool) -> Result<()> {
    ensure!(data.len() <= 100, "Too many dropdown choices");
    ensure!(!dropdown || !data.is_empty(), "A dropdown needs data");
    for c in data {
        ensure!(
            !c.title.is_empty() && c.title.len() <= 120 && c.value.len() <= 200,
            "Invalid dropdown choice"
        );
    }
    Ok(())
}
fn check_preferences(preferences: &[Preference]) -> Result<()> {
    ensure!(preferences.len() <= 40, "Too many preferences");
    let mut names = BTreeSet::new();
    for p in preferences {
        ensure!(
            valid_name(&p.name) && names.insert(&p.name),
            "Invalid or duplicate preference name"
        );
        ensure!(
            ["textfield", "password", "checkbox", "dropdown"].contains(&p.kind.as_str()),
            "Unknown preference type: {}",
            p.kind
        );
        ensure!(
            !p.title.is_empty() && p.title.len() <= 120 && p.description.len() <= 500 && p.label.len() <= 120,
            "Invalid preference title or description"
        );
        check_choices(&p.data, p.kind == "dropdown")?;
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub struct Extension {
    pub manifest: Manifest,
    pub directory: PathBuf,
    /// Shipped with Lumen: off until the user turns it on, and never removed.
    pub builtin: bool,
}
/// IDs with this prefix belong to extensions that ship with Lumen.
pub const BUILTIN_PREFIX: &str = "lumen.";
pub fn valid_icon(icon: &str) -> bool {
    icon.len() <= 100
        && icon
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
/// Where the bundled extensions live. Installs keep them beside the binary
/// (`<prefix>/bin` and `<prefix>/share/gnome-launcher/builtin`); debug builds
/// fall back to the repository so `cargo run` finds them.
pub fn builtin_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("LUMEN_BUILTIN_DIR") {
        return Some(PathBuf::from(dir));
    }
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(Path::to_path_buf))
        .map(|prefix| prefix.join("share/gnome-launcher/builtin/extensions"))
        .filter(|dir| dir.is_dir());
    #[cfg(debug_assertions)]
    let installed = installed.or_else(|| {
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions")).filter(|d| d.is_dir())
    });
    installed
}
pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'.')
        && !id.starts_with('.')
        && !id.ends_with('.')
        && !id.contains("..")
}
pub fn load(directory: &Path) -> Result<Extension> {
    let bytes = fs::read(directory.join("extension.json"))?;
    ensure!(bytes.len() <= 64 * 1024, "Manifest is too large");
    let manifest: Manifest = serde_json::from_slice(&bytes).context("Invalid extension.json")?;
    ensure!(valid_id(&manifest.id), "Invalid extension ID");
    ensure!(
        manifest.sdk_version == 1,
        "This extension requires a different SDK version"
    );
    ensure!(
        !manifest.name.is_empty() && manifest.name.len() <= 120,
        "Invalid extension name"
    );
    ensure!(
        !manifest.version.is_empty() && manifest.version.len() < 64,
        "Invalid version"
    );
    ensure!(
        safe_relative(Path::new(&manifest.entry)),
        "Entry must be a relative path"
    );
    let root = directory.canonicalize()?;
    let entry = directory
        .join(&manifest.entry)
        .canonicalize()
        .context("Extension entry is missing")?;
    ensure!(
        entry.starts_with(&root) && entry.is_file(),
        "Entry escapes the extension directory"
    );
    ensure!(
        !manifest.commands.is_empty() && manifest.commands.len() <= 50,
        "Expected 1–50 commands"
    );
    let mut ids = BTreeSet::new();
    for command in &manifest.commands {
        ensure!(
            valid_id(&command.id) && ids.insert(&command.id),
            "Invalid or duplicate command ID"
        );
        ensure!(
            !command.title.is_empty() && command.title.len() <= 160,
            "Invalid command title"
        );
        ensure!(valid_icon(&command.icon), "Invalid command icon name");
        ensure!(command.arguments.len() <= 3, "A command takes at most 3 arguments");
        let mut names = BTreeSet::new();
        for a in &command.arguments {
            ensure!(
                valid_name(&a.name) && names.insert(&a.name),
                "Invalid or duplicate argument name"
            );
            ensure!(
                ["text", "password", "dropdown"].contains(&a.kind.as_str()),
                "Unknown argument type: {}",
                a.kind
            );
            ensure!(a.placeholder.len() <= 60, "Argument placeholder is too long");
            check_choices(&a.data, a.kind == "dropdown")?;
        }
        check_preferences(&command.preferences)?;
    }
    check_preferences(&manifest.preferences)?;
    for cap in &manifest.capabilities {
        ensure!(
            [
                "clipboard",
                "open",
                "notifications",
                "storage",
                "network",
                "filesystem",
                "process"
            ]
            .contains(&cap.as_str()),
            "Unknown capability: {cap}"
        );
    }
    Ok(Extension {
        manifest,
        directory: root,
        builtin: false,
    })
}
fn subdirectories(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            !e.file_name().to_string_lossy().starts_with('.')
                && e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}
pub fn discover(store: &store::Store) -> Vec<Extension> {
    discover_in(builtin_dir().as_deref(), store)
}
/// Built-ins load first so an installed package can never shadow one.
fn discover_in(builtin: Option<&Path>, store: &store::Store) -> Vec<Extension> {
    let installed = store::data_dir().join("extensions");
    recover_updates(&installed);
    let builtins = builtin.map(subdirectories).unwrap_or_default();
    let count = builtins.len();
    let mut roots = builtins;
    roots.extend(subdirectories(&installed));
    roots.extend(store.developers.iter().cloned());
    let mut seen = BTreeSet::new();
    roots
        .into_iter()
        .enumerate()
        .filter_map(|(index, p)| match load(&p) {
            Ok(mut ext) if seen.insert(ext.manifest.id.clone()) => {
                ext.builtin = index < count;
                Some(ext)
            }
            Ok(ext) => {
                eprintln!(
                    "Ignoring extension {}: {} is already loaded",
                    p.display(),
                    ext.manifest.id
                );
                None
            }
            Err(e) => {
                eprintln!("Ignoring extension {}: {e:#}", p.display());
                None
            }
        })
        .collect()
}

fn recover_updates(root: &Path) {
    for entry in fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(id) = name
            .strip_prefix('.')
            .and_then(|s| s.strip_suffix(".backup"))
        else {
            continue;
        };
        if !valid_id(id)
            || !entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
        {
            continue;
        }
        let target = root.join(id);
        let result = if !target.exists() && load(&entry.path()).is_ok_and(|e| e.manifest.id == id) {
            fs::rename(entry.path(), target)
        } else if load(&target).is_ok_and(|e| e.manifest.id == id) {
            fs::remove_dir_all(entry.path())
        } else {
            continue;
        };
        if let Err(e) = result {
            eprintln!("Could not recover extension update {id}: {e}");
        }
    }
}
pub fn download(source: &str, destination: &Path) -> Result<()> {
    let mut file = fs::File::create(destination)?;
    if source.starts_with("https://") {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(45)))
            .https_only(true)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .get(source)
            .call()
            .context("Could not download extension")?;
        std::io::copy(
            &mut response.body_mut().as_reader().take(MAX_PACKAGE + 1),
            &mut file,
        )?;
    } else {
        ensure!(!source.contains("://"), "Download URLs must use HTTPS");
        let mut input = fs::File::open(source).context("Cannot open extension package")?;
        std::io::copy(&mut (&mut input).take(MAX_PACKAGE + 1), &mut file)?;
    }
    ensure!(
        file.metadata()?.len() <= MAX_PACKAGE,
        "Package exceeds 20 MiB"
    );
    file.flush()?;
    Ok(())
}
pub fn unpack(package: &Path, target: &Path) -> Result<Extension> {
    ensure!(
        fs::metadata(package)?.len() <= MAX_PACKAGE,
        "Package exceeds 20 MiB"
    );
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(package)?));
    fs::create_dir_all(target)?;
    let mut total = 0u64;
    let mut count = 0usize;
    let mut paths = BTreeSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(safe_relative(&path), "Unsafe archive path");
        ensure!(paths.insert(path.clone()), "Duplicate archive path");
        count += 1;
        ensure!(count <= 2000, "Too many package entries");
        let ty = entry.header().entry_type();
        ensure!(
            ty.is_file() || ty.is_dir(),
            "Links and special files are not permitted"
        );
        total = total
            .checked_add(entry.size())
            .context("Invalid package size")?;
        ensure!(total <= MAX_UNPACKED, "Unpacked extension exceeds 50 MiB");
        ensure!(entry.unpack_in(target)?, "Path escaped package directory");
    }
    load(target)
}
pub fn install(staged: &Path, extensions: &Path, replace: bool) -> Result<()> {
    let extension = load(staged)?;
    ensure!(
        !extension.manifest.id.starts_with(BUILTIN_PREFIX),
        "The {BUILTIN_PREFIX} ID prefix is reserved for built-in extensions"
    );
    fs::create_dir_all(extensions)?;
    let target = extensions.join(&extension.manifest.id);
    let backup = extensions.join(format!(".{}.backup", extension.manifest.id));
    ensure!(
        !backup.exists(),
        "An interrupted update needs recovery: {}",
        backup.display()
    );
    if target.exists() {
        ensure!(replace, "Already installed; choose Replace to update");
        fs::rename(&target, &backup)?;
    }
    if let Err(e) = fs::rename(staged, &target) {
        if backup.exists() {
            fs::rename(&backup, &target)?;
        }
        return Err(e.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}
pub fn pack(directory: &Path, output: &Path) -> Result<()> {
    load(directory)?;
    ensure!(!output.exists(), "Output already exists");
    let root = directory.canonicalize()?;
    let parent = output.parent().unwrap_or(Path::new(".")).canonicalize()?;
    ensure!(
        !parent
            .join(output.file_name().context("Invalid output")?)
            .starts_with(&root),
        "Write packages outside the extension directory"
    );
    let writer = GzEncoder::new(fs::File::create(output)?, Compression::default());
    let mut tar = tar::Builder::new(writer);
    fn append(
        tar: &mut tar::Builder<GzEncoder<fs::File>>,
        root: &Path,
        dir: &Path,
        total: &mut u64,
        count: &mut usize,
    ) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            if name == "node_modules" || name == ".git" {
                continue;
            }
            let path = e.path();
            let metadata = e.metadata()?;
            ensure!(
                !e.file_type()?.is_symlink(),
                "Bundle real files, not symbolic links"
            );
            if metadata.is_dir() {
                append(tar, root, &path, total, count)?;
            } else if metadata.is_file() {
                *count += 1;
                *total += metadata.len();
                ensure!(
                    *count <= 2000 && *total <= MAX_UNPACKED,
                    "Extension is too large"
                );
                tar.append_path_with_name(&path, path.strip_prefix(root)?)?;
            } else {
                bail!("Unsupported file type");
            }
        }
        Ok(())
    }
    append(&mut tar, &root, &root, &mut 0, &mut 0)?;
    tar.into_inner()?.finish()?;
    ensure!(
        fs::metadata(output)?.len() <= MAX_PACKAGE,
        "Compressed package exceeds 20 MiB"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("extension.json"), r#"{"id":"test.example","name":"Example","version":"1.0.0","sdk_version":1,"entry":"index.mjs","commands":[{"id":"hello","title":"Hello"}]}"#).unwrap();
        fs::write(root.join("index.mjs"), "export {};").unwrap();
    }
    #[test]
    fn round_trip_and_explicit_update() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fixture(&root.join("source"));
        pack(&root.join("source"), &root.join("extension.tgz")).unwrap();
        unpack(&root.join("extension.tgz"), &root.join("stage")).unwrap();
        install(&root.join("stage"), &root.join("installed"), false).unwrap();
        unpack(&root.join("extension.tgz"), &root.join("stage")).unwrap();
        assert!(install(&root.join("stage"), &root.join("installed"), false).is_err());
        install(&root.join("stage"), &root.join("installed"), true).unwrap();
        assert!(load(&root.join("installed/test.example")).is_ok());
    }
    #[test]
    fn recovers_an_interrupted_update() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fixture(&root.join(".test.example.backup"));
        recover_updates(root);
        assert!(load(&root.join("test.example")).is_ok());
        assert!(!root.join(".test.example.backup").exists());
        fixture(&root.join(".test.example.backup"));
        recover_updates(root);
        assert!(!root.join(".test.example.backup").exists());
    }
    #[test]
    fn builtins_win_over_developer_folders_with_the_same_id() {
        let temp = tempfile::tempdir().unwrap();
        fixture(&temp.path().join("builtin/example"));
        fixture(&temp.path().join("developer"));
        let mut store = store::Store::default();
        store.developers.insert(temp.path().join("developer"));
        let found = discover_in(Some(&temp.path().join("builtin")), &store);
        let ext = found.iter().find(|e| e.manifest.id == "test.example").unwrap();
        assert!(ext.builtin);
        assert_eq!(found.iter().filter(|e| e.manifest.id == "test.example").count(), 1);
    }
    #[test]
    fn rejects_reserved_ids_and_bad_icons() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("stage");
        fixture(&root);
        let manifest = fs::read_to_string(root.join("extension.json")).unwrap();
        fs::write(root.join("extension.json"), manifest.replace("test.example", "lumen.fake")).unwrap();
        assert!(install(&root, &temp.path().join("installed"), false).is_err());
        let manifest = fs::read_to_string(root.join("extension.json")).unwrap();
        fs::write(root.join("extension.json"), manifest.replace(r#""title":"Hello""#, r#""title":"Hello","icon":"../x""#)).unwrap();
        assert!(load(&root).is_err());
    }
    #[test]
    fn rejects_traversal_links_and_bad_versions() {
        assert!(!safe_relative(Path::new("../oops")));
        assert!(!safe_relative(Path::new("/tmp/oops")));
        assert!(!valid_id("../test"));
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path());
        fs::remove_file(temp.path().join("index.mjs")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", temp.path().join("index.mjs")).unwrap();
        assert!(load(temp.path()).is_err());
        let file = fs::File::create(temp.path().join("bad.tgz")).unwrap();
        let mut tar = tar::Builder::new(GzEncoder::new(file, Compression::default()));
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_link_name("/tmp/oops").unwrap();
        h.set_cksum();
        tar.append_data(&mut h, "link", std::io::empty()).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(unpack(&temp.path().join("bad.tgz"), &temp.path().join("stage")).is_err());
    }
    #[test]
    fn manifest_preferences_and_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let write = |json: &str| {
            fs::write(root.join("extension.json"), json).unwrap();
            fs::write(root.join("index.mjs"), "export {};").unwrap();
            load(root)
        };
        let ok = write(r#"{"id":"test.args","name":"Args","version":"1","sdk_version":1,"entry":"index.mjs",
            "preferences":[{"name":"units","type":"dropdown","title":"Units","data":[{"title":"Metric","value":"metric"}]}],
            "commands":[{"id":"rotate","title":"Rotate","mode":"no-view",
              "arguments":[{"name":"angle","placeholder":"Angle","required":true}],
              "preferences":[{"name":"radians","type":"checkbox","title":"Unit","label":"Use radians"}]}]}"#)
        .unwrap();
        let command = &ok.manifest.commands[0];
        assert_eq!(command.mode, Mode::NoView);
        assert_eq!(command.arguments[0].kind, "text");
        assert_eq!(ok.manifest.preferences[0].fallback(), serde_json::json!("metric"));
        assert_eq!(command.preferences[0].fallback(), serde_json::json!(false));
        assert!(write(r#"{"id":"test.args","name":"Args","version":"1","sdk_version":1,"entry":"index.mjs",
            "commands":[{"id":"a","title":"A","arguments":[{"name":"a"},{"name":"b"},{"name":"c"},{"name":"d"}]}]}"#).is_err());
        assert!(write(r#"{"id":"test.args","name":"Args","version":"1","sdk_version":1,"entry":"index.mjs",
            "preferences":[{"name":"x","type":"slider","title":"X"}],"commands":[{"id":"a","title":"A"}]}"#).is_err());
        assert!(write(r#"{"id":"test.args","name":"Args","version":"1","sdk_version":1,"entry":"index.mjs",
            "commands":[{"id":"a","title":"A","mode":"menu-bar"}]}"#).is_err());
    }
    #[test]
    fn bundled_extensions_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions");
        let found = discover_in(Some(&root), &store::Store::default());
        for id in ["lumen.timers", "lumen.speedtest", "lumen.port-manager"] {
            assert!(found.iter().any(|e| e.manifest.id == id && e.builtin), "{id}");
        }
    }
}
