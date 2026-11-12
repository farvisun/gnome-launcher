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
const TIMEOUT: Duration = Duration::from_secs(10);
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
    fn send(&mut self, value: &Value, cancelled: &AtomicBool, started: Instant) -> Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        ensure!(
            bytes.len() < MAX_FRAME as usize,
            "Request exceeds protocol frame limit"
        );
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            ensure!(!cancelled.load(Ordering::Relaxed), "Cancelled");
            ensure!(
                started.elapsed() < TIMEOUT,
                "Extension timed out after 10 seconds"
            );
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
        let started = Instant::now();
        self.send(
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            cancelled,
            started,
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
            ensure!(
                started.elapsed() < TIMEOUT,
                "Extension timed out after 10 seconds"
            );
            let value = match self.output.recv_timeout(Duration::from_millis(25)) {
                Ok(v) => v,
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
                    let result = host(ext, method, &value["params"], event);
                    let response = match result {
                        Ok(result) => json!({"jsonrpc":"2.0", "id":value["id"], "result":result}),
                        Err(error) => {
                            json!({"jsonrpc":"2.0", "id":value["id"], "error":{"code":-32000,"message":error.to_string()}})
                        }
                    };
                    self.send(&response, cancelled, started)?;
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
    match view["type"].as_str() {
        Some("list") => {
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
            }
        }
        Some("detail") => {
            ensure!(
                view["text"].as_str().is_some_and(|s| s.len() <= 100_000),
                "Detail requires text up to 100 KB"
            );
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
                ensure!(
                    ["text", "password", "boolean"]
                        .contains(&field["type"].as_str().unwrap_or("text")),
                    "Unsupported form field type"
                );
                ensure!(field["label"].is_string(), "Form field needs a label");
            }
        }
        _ => bail!("Expected a list, detail, form, or null view"),
    }
    Ok(())
}
fn host(ext: &Extension, method: &str, params: &Value, event: &impl Fn(Value)) -> Result<Value> {
    let required = match method {
        "host.copy" => "clipboard",
        "host.open" => "open",
        "host.notify" => "notifications",
        "host.storage.get" | "host.storage.set" => "storage",
        _ => bail!("Unknown host method"),
    };
    ensure!(
        ext.manifest.capabilities.iter().any(|c| c == required),
        "Undeclared capability: {required}"
    );
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
}
