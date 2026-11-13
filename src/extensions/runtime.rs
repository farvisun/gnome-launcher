use super::package::Extension;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const MAX_FRAME: u64 = 1024 * 1024;
/// A request fails when the worker sends nothing for this long. Progress
/// updates and host calls count as activity, so long tasks stay alive.
#[cfg(not(test))]
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const IDLE_TIMEOUT: Duration = Duration::from_millis(1000);
/// No request may run longer than this, however chatty the worker is.
#[cfg(not(test))]
const MAX_REQUEST: Duration = Duration::from_secs(120);
#[cfg(test)]
const MAX_REQUEST: Duration = Duration::from_millis(3000);
fn check_deadline(started: Instant, active: Instant) -> Result<()> {
    ensure!(
        started.elapsed() < MAX_REQUEST,
        "Extension exceeded {} seconds",
        MAX_REQUEST.as_secs()
    );
    ensure!(
        active.elapsed() < IDLE_TIMEOUT,
        "Extension did not respond for {} seconds",
        IDLE_TIMEOUT.as_secs()
    );
    Ok(())
}
/// Host calls the GTK thread must answer (a file chooser, the clipboard)
/// wait here for `answer`, keyed by the `ask` event's id.
type Reply = mpsc::Sender<std::result::Result<Value, String>>;
static ASKS: Mutex<BTreeMap<u64, Reply>> = Mutex::new(BTreeMap::new());
static NEXT_ASK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Delivers the GTK thread's answer to a waiting `ask` host call.
pub fn answer(id: u64, result: std::result::Result<Value, String>) {
    if let Some(reply) = ASKS.lock().ok().and_then(|mut asks| asks.remove(&id)) {
        let _ = reply.send(result);
    }
}
/// Emits an `ask` event and blocks until the GTK thread answers or the request
/// is cancelled. The wait does not count towards the request's time limits.
fn ask(
    method: &str,
    params: &Value,
    cancelled: &AtomicBool,
    event: &impl Fn(Value),
) -> Result<Value> {
    let id = NEXT_ASK.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    ASKS.lock()
        .map_err(|_| anyhow::anyhow!("Host unavailable"))?
        .insert(id, tx);
    event(json!({"type":"ask", "id":id, "method":method, "params":params}));
    loop {
        if cancelled.load(Ordering::Relaxed) {
            if let Ok(mut asks) = ASKS.lock() {
                asks.remove(&id);
            }
            bail!("Cancelled");
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result.map_err(anyhow::Error::msg),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => bail!("Host closed the request"),
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    processes: Mutex<BTreeMap<String, Process>>,
}
struct Process {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Value>,
    touched: Instant,
    next_id: u64,
    directory: std::path::PathBuf,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Process {
    fn start(ext: &Extension) -> Result<Self> {
        let node = std::env::var_os("LUMEN_NODE").unwrap_or_else(|| "node".into());
        let mut child = Command::new(node)
            .arg(&ext.manifest.entry)
            .current_dir(&ext.directory)
            .env("LUMEN_EXTENSION_ID", &ext.manifest.id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Could not start extension; install Node.js 22 or newer")?;
        let input = child.stdin.take().context("Missing extension input")?;
        // A worker that stops reading must not block cancellation or app shutdown.
        let fd = input.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                let error = std::io::Error::last_os_error();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
        let output = child.stdout.take().context("Missing extension output")?;
        let (tx, rx) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                match reader
                    .by_ref()
                    .take(MAX_FRAME + 1)
                    .read_until(b'\n', &mut bytes)
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) if bytes.len() as u64 > MAX_FRAME => break,
                    Ok(_) => match serde_json::from_slice(&bytes) {
                        Ok(value) => {
                            if tx.send(value).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
        });
        Ok(Self {
            child,
            input,
            output: rx,
            touched: Instant::now(),
            next_id: 1,
            directory: ext.directory.clone(),
        })
    }
    fn send(
        &mut self,
        value: &Value,
        cancelled: &AtomicBool,
        started: Instant,
        active: Instant,
    ) -> Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        ensure!(
            bytes.len() < MAX_FRAME as usize,
            "Request exceeds protocol frame limit"
        );
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            ensure!(!cancelled.load(Ordering::Relaxed), "Cancelled");
            check_deadline(started, active)?;
            match self.input.write(&bytes[offset..]) {
                Ok(0) => bail!("Extension closed its input"),
                Ok(count) => offset += count,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn request(
        &mut self,
        ext: &Extension,
        method: &str,
        params: Value,
        cancelled: &AtomicBool,
        event: &impl Fn(Value),
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut started = Instant::now();
        let mut active = started;
        self.send(
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            cancelled,
            started,
            active,
        )?;
        let mut messages = 0;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                let notification = format!(
                    "{{\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{{\"id\":{id}}}}}\n"
                );
                let _ = self.input.write(notification.as_bytes());
                bail!("Cancelled");
            }
            check_deadline(started, active)?;
            let value = match self.output.recv_timeout(Duration::from_millis(25)) {
                Ok(v) => {
                    active = Instant::now();
                    v
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => bail!("Extension exited or sent invalid protocol data; check its logs"),
            };
            messages += 1;
            ensure!(messages <= 500, "Extension sent too many messages");
            ensure!(value["jsonrpc"] == "2.0", "Unsupported extension protocol");
            if let Some(method) = value["method"].as_str() {
                if method == "view.update" && value["params"]["request_id"] == id {
                    validate_view(&value["params"]["view"])?;
                    event(json!({"type":"view", "view":value["params"]["view"]}));
                } else if method.starts_with("host.") && !value["id"].is_null() {
                    let asked = Instant::now();
                    let result = host(ext, method, &value["params"], cancelled, event);
                    // Time spent waiting for the user (a file chooser) is not the worker's.
                    if ASKING.contains(&method) {
                        started += asked.elapsed();
                        active = Instant::now();
                    }
                    let response = match result {
                        Ok(result) => json!({"jsonrpc":"2.0", "id":value["id"], "result":result}),
                        Err(error) => {
                            json!({"jsonrpc":"2.0", "id":value["id"], "error":{"code":-32000,"message":error.to_string()}})
                        }
                    };
                    self.send(&response, cancelled, started, active)?;
                }
            } else if value["id"] == id {
                self.touched = Instant::now();
                if !value["error"].is_null() {
                    bail!(
                        "{}",
                        value["error"]["message"]
                            .as_str()
                            .unwrap_or("Extension error")
                    );
                }
                let view = value["result"].clone();
                validate_view(&view)?;
                return Ok(view);
            }
        }
    }
}
impl Runtime {
    pub fn shutdown(&self) {
        if let Ok(mut processes) = self.processes.lock() {
            processes.clear();
        }
    }
    pub fn request(
        &self,
        ext: &Extension,
        method: &str,
        params: Value,
        cancelled: Arc<AtomicBool>,
        event: impl Fn(Value),
    ) -> Result<Value> {
        let mut processes = self
            .processes
            .lock()
            .map_err(|_| anyhow::anyhow!("Extension runtime unavailable"))?;
        if cancelled.load(Ordering::Relaxed) {
            bail!("Cancelled");
        }
        let id = &ext.manifest.id;
        if processes
            .get(id)
            .is_some_and(|p| p.directory != ext.directory)
        {
            processes.remove(id);
        }
        if !processes.contains_key(id) {
            processes.insert(id.clone(), Process::start(ext)?);
        }
        let result = processes
            .get_mut(id)
            .unwrap()
            .request(ext, method, params, &cancelled, &event);
        if result.is_err() {
            processes.remove(id);
        }
        result
    }
    pub fn stop(&self, id: &str) {
        if let Ok(mut processes) = self.processes.try_lock() {
            processes.remove(id);
        }
    }
    pub fn reap(&self) {
        if let Ok(mut processes) = self.processes.try_lock() {
            processes.retain(|_, p| {
                p.touched.elapsed() < Duration::from_secs(60)
                    && p.child.try_wait().ok().flatten().is_none()
            });
        }
    }
}
pub fn validate_view(view: &Value) -> Result<()> {
    if view.is_null() {
        return Ok(());
    }
    ensure!(
        view["push"].is_null() || view["push"].is_boolean(),
        "push must be true or false"
    );
    ensure!(
        view["selection"].is_null() || view["selection"].is_boolean(),
        "selection must be true or false"
    );
    ensure!(
        view["context"].is_null() || serde_json::to_vec(&view["context"])?.len() <= 4096,
        "View context exceeds 4 KB"
    );
    optional_text(&view["title"], 200, "view title")?;
    match view["type"].as_str() {
        Some(kind @ ("list" | "grid")) => {
            let items = view["items"].as_array().context("List needs items")?;
            ensure!(
                items.len() <= 100,
                "Extensions may return at most 100 items"
            );
            let mut ids = std::collections::BTreeSet::new();
            for item in items {
                let id = item["id"].as_str().context("List item needs an ID")?;
                ensure!(
                    ids.insert(id) && id.len() <= 200,
                    "Invalid or duplicate list item ID"
                );
                ensure!(
                    item["title"].as_str().is_some_and(|s| s.len() <= 1000),
                    "Invalid item title"
                );
                optional_text(&item["subtitle"], 1000, "item subtitle")?;
                if kind == "list" {
                    if let Some(icon) = item["icon"].as_str().filter(|i| i.starts_with('/')) {
                        valid_image(icon)?;
                    }
                    validate_decoration(item)?;
                } else {
                    optional_text(&item["section"], 100, "section")?;
                    optional_text(&item["tooltip"], 300, "tooltip")?;
                    let content = &item["content"];
                    optional_color(&content["tint"])?;
                    optional_text(&content["icon"], 1000, "grid icon")?;
                    if let Some(icon) = content["icon"].as_str().filter(|i| i.starts_with('/')) {
                        valid_image(icon)?;
                    }
                    if let Some(image) = content["image"].as_str() {
                        valid_image(image)?;
                    } else {
                        ensure!(content["image"].is_null(), "Grid image must be a path");
                    }
                }
            }
            if let Some(refresh) = view.get("refresh") {
                ensure!(
                    refresh.as_u64().is_some_and(|ms| (500..=60_000).contains(&ms)),
                    "List refresh must be 500–60000 ms"
                );
            }
            if kind == "grid" {
                ensure!(
                    view["columns"].is_null()
                        || view["columns"].as_u64().is_some_and(|c| (3..=8).contains(&c)),
                    "Grid columns must be 3–8"
                );
                ensure!(
                    view["aspect"].is_null()
                        || ["1", "3/2", "2/3", "4/3", "3/4", "16/9", "9/16"]
                            .contains(&view["aspect"].as_str().unwrap_or("")),
                    "Grid aspect must be 1, 3/2, 2/3, 4/3, 3/4, 16/9 or 9/16"
                );
            }
            optional_text(&view["placeholder"], 200, "placeholder")?;
            optional_text(&view["empty"]["title"], 200, "empty view title")?;
            optional_text(&view["empty"]["description"], 1000, "empty view description")?;
        }
        Some("detail") => {
            ensure!(
                view["text"].as_str().is_some_and(|s| s.len() <= 100_000),
                "Detail requires text up to 100 KB"
            );
            if let Some(image) = view["image"].as_str() {
                valid_image(image)?;
            }
            validate_metadata(&view["metadata"])?;
        }
        Some("form") => {
            let fields = view["fields"].as_array().context("Form needs fields")?;
            ensure!(fields.len() <= 30, "Form exceeds 30 fields");
            let mut ids = std::collections::BTreeSet::new();
            for field in fields {
                ensure!(
                    field["id"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 100 && ids.insert(s)),
                    "Invalid field ID"
                );
                let kind = field["type"].as_str().unwrap_or("text");
                ensure!(
                    ["text", "password", "boolean", "dropdown", "description"].contains(&kind),
                    "Unsupported form field type"
                );
                ensure!(field["label"].is_string(), "Form field needs a label");
                optional_text(&field["info"], 1000, "field info")?;
                if kind == "description" {
                    ensure!(
                        field["text"].as_str().is_some_and(|t| t.len() <= 5000),
                        "A description field needs text"
                    );
                }
                if kind == "dropdown" {
                    let options = field["options"].as_array().context("A dropdown needs options")?;
                    ensure!(
                        !options.is_empty()
                            && options.len() <= 100
                            && options.iter().all(|o| {
                                o["value"].is_string()
                                    && o["title"].as_str().is_some_and(|t| t.len() <= 200)
                            }),
                        "Dropdown options are 1–100 {{value, title}} pairs"
                    );
                }
            }
        }
        _ => bail!("Expected a list, grid, detail, form, or null view"),
    }
    Ok(())
}
/// Images are local files only: no URLs, so a view cannot make Lumen fetch anything.
fn valid_image(path: &str) -> Result<()> {
    let path = std::path::Path::new(path);
    ensure!(path.is_absolute(), "Image must be an absolute path");
    let meta = std::fs::metadata(path)
        .with_context(|| format!("Missing image {}", path.display()))?;
    ensure!(
        meta.is_file() && meta.len() <= 10 * 1024 * 1024,
        "Image must be a file up to 10 MB"
    );
    Ok(())
}
fn validate_metadata(metadata: &Value) -> Result<()> {
    if metadata.is_null() {
        return Ok(());
    }
    let rows = metadata.as_array().context("Metadata must be a list")?;
    ensure!(rows.len() <= 40, "At most 40 metadata rows");
    for row in rows {
        optional_text(&row["label"], 100, "metadata label")?;
        optional_text(&row["text"], 2000, "metadata text")?;
        optional_color(&row["color"])?;
        for tag in row["tags"].as_array().into_iter().flatten().take(21) {
            optional_text(&tag["text"], 60, "metadata tag")?;
            optional_color(&tag["color"])?;
        }
        ensure!(
            row["tags"].as_array().is_none_or(|t| t.len() <= 20),
            "At most 20 tags per row"
        );
    }
    Ok(())
}
fn optional_text(value: &Value, limit: usize, what: &str) -> Result<()> {
    ensure!(
        value.is_null() || value.as_str().is_some_and(|s| s.len() <= limit),
        "Invalid {what}"
    );
    Ok(())
}
fn optional_color(value: &Value) -> Result<()> {
    ensure!(
        value.is_null()
            || value
                .as_str()
                .is_some_and(|c| crate::ui::COLORS.contains(&c)),
        "Unknown colour; use {}",
        crate::ui::COLORS.join(", ")
    );
    Ok(())
}
/// Sections, tints, accessories, progress and the side-pane detail of one list item.
fn validate_decoration(item: &Value) -> Result<()> {
    optional_text(&item["section"], 100, "section")?;
    optional_color(&item["tint"])?;
    ensure!(
        item["progress"].is_null()
            || item["progress"].as_f64().is_some_and(|p| (0. ..=1.).contains(&p)),
        "Progress must be between 0 and 1"
    );
    if !item["accessories"].is_null() {
        let accessories = item["accessories"].as_array().context("Accessories must be a list")?;
        ensure!(accessories.len() <= 6, "At most 6 accessories per item");
        for accessory in accessories {
            optional_text(&accessory["text"], 100, "accessory text")?;
            optional_text(&accessory["tag"], 60, "accessory tag")?;
            optional_text(&accessory["tooltip"], 300, "accessory tooltip")?;
            if let Some(icon) = accessory["icon"].as_str().filter(|i| i.starts_with('/')) {
                valid_image(icon)?;
            }
            optional_color(&accessory["color"])?;
        }
    }
    let detail = &item["detail"];
    if detail.is_null() {
        return Ok(());
    }
    ensure!(detail.is_object(), "Item detail must be an object");
    optional_text(&detail["text"], 20_000, "detail text")?;
    optional_text(&detail["hero"]["value"], 40, "hero value")?;
    optional_text(&detail["hero"]["unit"], 20, "hero unit")?;
    optional_text(&detail["hero"]["caption"], 200, "hero caption")?;
    optional_color(&detail["hero"]["color"])?;
    optional_color(&detail["chart"]["color"])?;
    if let Some(values) = detail["chart"]["values"].as_array() {
        ensure!(
            values.len() <= 200 && values.iter().all(|v| v.as_f64().is_some_and(f64::is_finite)),
            "Chart needs up to 200 finite numbers"
        );
    }
    if let Some(image) = detail["image"].as_str() {
        valid_image(image)?;
    }
    validate_metadata(&detail["metadata"])?;
    Ok(())
}
/// Host methods answered by the GTK thread, possibly after the user acts.
const ASKING: [&str; 2] = ["host.readClipboard", "host.chooseFiles"];
fn host(
    ext: &Extension,
    method: &str,
    params: &Value,
    cancelled: &AtomicBool,
    event: &impl Fn(Value),
) -> Result<Value> {
    let required = match method {
        "host.copy" | "host.copyFile" | "host.readClipboard" => Some("clipboard"),
        "host.open" => Some("open"),
        "host.notify" => Some("notifications"),
        "host.storage.get" | "host.storage.set" => Some("storage"),
        "host.toast" | "host.hud" | "host.closeWindow" | "host.openPreferences"
        | "host.chooseFiles" => None,
        _ => bail!("Unknown host method"),
    };
    if let Some(required) = required {
        ensure!(
            ext.manifest.capabilities.iter().any(|c| c == required),
            "Undeclared capability: {required}"
        );
    }
    match method {
        "host.readClipboard" => return ask(method, params, cancelled, event),
        "host.chooseFiles" => {
            optional_text(&params["title"], 200, "chooser title")?;
            ensure!(
                params["mimeTypes"].as_array().is_none_or(|m| m.len() <= 40 && m.iter().all(Value::is_string)),
                "mimeTypes must be a list of strings"
            );
            return ask(method, params, cancelled, event);
        }
        "host.toast" => {
            ensure!(
                params["title"].as_str().is_some_and(|t| t.len() <= 200),
                "A toast needs a title"
            );
            optional_text(&params["message"], 1000, "toast message")?;
            ensure!(
                params["style"].is_null()
                    || ["success", "failure", "animated"].contains(&params["style"].as_str().unwrap_or("")),
                "Toast style is success, failure or animated"
            );
            event(json!({"type":"effect", "method":method, "params":params}));
            return Ok(Value::Null);
        }
        "host.closeWindow" | "host.openPreferences" => {
            event(json!({"type":"effect", "method":method, "params":{}}));
            return Ok(Value::Null);
        }
        "host.copyFile" => {
            let path = params["path"].as_str().context("Missing path")?;
            ensure!(
                std::path::Path::new(path).is_absolute() && std::path::Path::new(path).is_file(),
                "copyFile needs an absolute path to a file"
            );
            event(json!({"type":"effect", "method":method, "params":{"path":path}}));
            return Ok(Value::Null);
        }
        _ => {}
    }
    if method.starts_with("host.storage.") {
        let path = crate::store::data_dir()
            .join("extension-storage")
            .join(format!("{}.json", ext.manifest.id));
        let mut data: BTreeMap<String, Value> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Extension storage is corrupt")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        let key = params["key"].as_str().context("Missing storage key")?;
        ensure!(key.len() <= 200, "Storage key is too long");
        if method == "host.storage.get" {
            return Ok(data.get(key).cloned().unwrap_or(Value::Null));
        }
        data.insert(key.into(), params["value"].clone());
        ensure!(
            serde_json::to_vec(&data)?.len() <= 1024 * 1024,
            "Storage exceeds 1 MiB"
        );
        crate::store::write_json(&path, &data)?;
    } else {
        let text = params["text"].as_str().context("Missing text")?;
        ensure!(text.len() <= 100_000, "Host action exceeds size limit");
        // host.hud: shown by the shell after Lumen closes.
        if method == "host.open" {
            ensure!(
                ["https://", "http://", "file://", "mailto:"]
                    .iter()
                    .any(|p| text.starts_with(p)),
                "Unsupported URL scheme"
            );
        }
        event(json!({"type":"effect", "method":method, "text":text}));
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_results_and_rejects_duplicate_ids() {
        assert!(validate_view(&json!({"type":"detail","text":"Hello"})).is_ok());
        assert!(
            validate_view(
                &json!({"type":"list","items":[{"id":"1","title":"A"},{"id":"1","title":"B"}]})
            )
            .is_err()
        );
        assert!(validate_view(&json!({"type":"webview","url":"..."})).is_err());
        let rich = json!({"type":"list","refresh":1000,"items":[{"id":"a","title":"A","section":"Running","tint":"green","progress":0.5,
            "accessories":[{"text":"5 min"},{"tag":"Running","color":"yellow"}],
            "detail":{"hero":{"value":"12","unit":"Mbps","color":"blue"},"chart":{"values":[1,2,3]},"metadata":[{"label":"PID","text":"1"},{"separator":true},{"label":"Ports","tags":[{"text":"80","color":"orange"}]}]}}]});
        assert!(validate_view(&rich).is_ok());
        let mut bad = rich.clone();
        bad["items"][0]["tint"] = json!("pink");
        assert!(validate_view(&bad).is_err());
        let mut bad = rich.clone();
        bad["items"][0]["progress"] = json!(2);
        assert!(validate_view(&bad).is_err());
        let mut bad = rich;
        bad["refresh"] = json!(10);
        assert!(validate_view(&bad).is_err());
    }
    #[test]
    fn validates_grids_images_and_forms() {
        let image = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/assets/lumen-timers-light.png");
        let image = image.to_str().unwrap();
        let grid = json!({"type":"grid","columns":5,"aspect":"1","push":true,"context":{"day":"x"},"selection":true,
            "items":[{"id":"a","title":"Blur","section":"Blur","content":{"image":image}},{"id":"b","title":"Icon","content":{"icon":"weather-clear-symbolic","tint":"yellow"}}]});
        assert!(validate_view(&grid).is_ok());
        let mut bad = grid.clone();
        bad["items"][0]["content"]["image"] = json!("https://example.com/a.png");
        assert!(validate_view(&bad).is_err());
        let mut bad = grid.clone();
        bad["columns"] = json!(12);
        assert!(validate_view(&bad).is_err());
        let mut bad = grid;
        bad["context"] = json!("x".repeat(5000));
        assert!(validate_view(&bad).is_err());
        assert!(validate_view(&json!({"type":"list","items":[{"id":"a","title":"A","icon":image,"detail":{"image":image}}]})).is_ok());
        assert!(validate_view(&json!({"type":"list","items":[{"id":"a","title":"A","icon":"/no/such.png"}]})).is_err());
        let form = json!({"type":"form","title":"F","fields":[
            {"id":"f","label":"Format","type":"dropdown","options":[{"value":"png","title":"PNG"}],"info":"Pick one"},
            {"id":"d","label":"","type":"description","text":"Help"}]});
        assert!(validate_view(&form).is_ok());
        assert!(validate_view(&json!({"type":"form","fields":[{"id":"f","label":"F","type":"dropdown"}]})).is_err());
        assert!(validate_view(&json!({"type":"detail","text":"x","image":image,"metadata":[{"label":"A","text":"b"}]})).is_ok());
    }
    #[test]
    fn runs_sample_and_survives_a_crash() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links");
        let ext = super::super::package::load(&path).unwrap();
        let runtime = Runtime::default();
        let response = runtime
            .request(
                &ext,
                "command.run",
                json!({"command":"links","query":""}),
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
            .unwrap();
        assert_eq!(response["type"], "list");
        let result = runtime.request(
            &ext,
            "command.run",
            json!({"command":"missing"}),
            Arc::new(AtomicBool::new(false)),
            |_| {},
        );
        assert!(result.is_err());
        assert!(
            runtime
                .request(
                    &ext,
                    "command.run",
                    json!({"command":"links","query":""}),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .is_ok()
        );
    }
    #[test]
    fn restarts_after_process_exit_and_cancels_a_hung_worker() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("worker.mjs"), r#"
import {createInterface} from 'node:readline';
createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line);
 if(m.params.mode==='crash') process.exit(3);
 if(m.params.mode==='hang') return;
 process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result:{type:'detail',text:'Recovered'}})+'\n');
});
"#).unwrap();
        let mut ext = super::super::package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links"),
        )
        .unwrap();
        ext.directory = root.path().to_owned();
        ext.manifest.entry = "worker.mjs".into();
        let runtime = Arc::new(Runtime::default());
        assert!(
            runtime
                .request(
                    &ext,
                    "command.run",
                    json!({"mode":"crash"}),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .is_err()
        );
        assert_eq!(
            runtime
                .request(
                    &ext,
                    "command.run",
                    json!({}),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .unwrap()["text"],
            "Recovered"
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let worker = runtime.clone();
        let ext_clone = ext.clone();
        let task = std::thread::spawn(move || {
            worker.request(
                &ext_clone,
                "command.run",
                json!({"mode":"hang"}),
                flag,
                |_| {},
            )
        });
        std::thread::sleep(Duration::from_millis(80));
        cancel.store(true, Ordering::Relaxed);
        assert!(task.join().unwrap().is_err());
        assert_eq!(
            runtime
                .request(
                    &ext,
                    "command.run",
                    json!({}),
                    Arc::new(AtomicBool::new(false)),
                    |_| {}
                )
                .unwrap()["text"],
            "Recovered"
        );
    }
    fn worker(source: &str) -> (tempfile::TempDir, Extension) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("worker.mjs"), source).unwrap();
        let mut ext = super::super::package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links"),
        )
        .unwrap();
        ext.directory = root.path().to_owned();
        ext.manifest.entry = "worker.mjs".into();
        (root, ext)
    }
    const TICKING: &str = r#"
import {createInterface} from 'node:readline';
createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line); let n=0;
 const timer=setInterval(()=>{
  if(++n<m.params.ticks){process.stdout.write(JSON.stringify({jsonrpc:'2.0',method:'view.update',params:{request_id:m.id,view:{type:'detail',text:String(n)}}})+'\n');return;}
  clearInterval(timer);
  process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result:{type:'detail',text:'Done'}})+'\n');
 },m.params.every);
});
"#;
    #[test]
    fn progress_updates_extend_the_idle_timeout_up_to_the_hard_cap() {
        let (_root, ext) = worker(TICKING);
        let runtime = Runtime::default();
        let run = |ticks: u32, every: u32| {
            runtime.request(
                &ext,
                "command.run",
                json!({"ticks":ticks,"every":every}),
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
        };
        // Warm up so Node's start-up time does not count against the limits below.
        run(1, 10).unwrap();
        // Three 700 ms ticks outlast the 1 s idle limit only because each one is activity.
        assert_eq!(run(3, 700).unwrap()["text"], "Done");
        // A single silent 1.3 s gap is idle for too long.
        assert!(run(1, 1300).is_err());
        // Constant updates still stop at the 3 s cap.
        assert!(run(20, 200).is_err());
    }
    #[test]
    fn cancels_when_worker_does_not_read_stdin() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("worker.mjs"), "setInterval(()=>{},1000);").unwrap();
        let mut ext = super::super::package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links"),
        )
        .unwrap();
        ext.directory = root.path().to_owned();
        ext.manifest.entry = "worker.mjs".into();
        let runtime = Runtime::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let task = std::thread::spawn(move || {
            runtime.request(
                &ext,
                "command.run",
                json!({"large":"x".repeat(500_000)}),
                flag,
                |_| {},
            )
        });
        std::thread::sleep(Duration::from_millis(80));
        let started = Instant::now();
        cancel.store(true, Ordering::Relaxed);
        assert!(task.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn bundled_port_manager_lists_ports_through_the_runtime() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/port-manager");
        let ext = super::super::package::load(&path).unwrap();
        let view = Runtime::default()
            .request(
                &ext,
                "command.run",
                json!({"command":"open-ports","query":""}),
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
            .unwrap();
        assert_eq!(view["type"], "list");
    }
}
