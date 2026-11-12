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
}
#[derive(Clone, Debug)]
pub struct Extension {
    pub manifest: Manifest,
    pub directory: PathBuf,
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
    }
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
    })
}
pub fn discover(store: &store::Store) -> Vec<Extension> {
    let installed = store::data_dir().join("extensions");
    recover_updates(&installed);
    let mut roots: Vec<PathBuf> = fs::read_dir(&installed)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            !e.file_name().to_string_lossy().starts_with('.')
                && e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())
        })
        .map(|e| e.path())
        .collect();
    roots.extend(store.developers.iter().cloned());
    let mut seen = BTreeSet::new();
    roots
        .into_iter()
        .filter_map(|p| match load(&p) {
            Ok(ext) if seen.insert(ext.manifest.id.clone()) => Some(ext),
            Ok(_) => None,
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
}
