//! TinySPARQL is loaded dynamically so a missing indexer never prevents app search.
//! The bus connection is thread-safe and reused across queries; cursors stay on
//! the querying worker. Only owned strings escape.
use crate::model::{Item, Kind};
use anyhow::{Context, Result, bail};
use gtk::{
    gio::{self, prelude::*},
    glib::{self, translate::*},
};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    sync::{Mutex, OnceLock},
};

type Connect = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *mut c_void,
    *mut *mut glib::ffi::GError,
) -> *mut c_void;
type Query = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *mut gio::ffi::GCancellable,
    *mut *mut glib::ffi::GError,
) -> *mut c_void;
type Next = unsafe extern "C" fn(
    *mut c_void,
    *mut gio::ffi::GCancellable,
    *mut *mut glib::ffi::GError,
) -> c_int;
type GetString = unsafe extern "C" fn(*mut c_void, c_int, *mut isize) -> *const c_char;

struct Object(*mut c_void);
// TrackerSparqlConnection is documented as thread-safe; cursors never leave a worker.
unsafe impl Send for Object {}
impl Object {
    fn share(&self) -> Self {
        unsafe { glib::gobject_ffi::g_object_ref(self.0.cast()) };
        Self(self.0)
    }
}
/// Opening a LocalSearch bus connection costs a D-Bus round trip and schema
/// introspection; keeping one makes each keystroke's query noticeably faster.
static CONNECTION: Mutex<Option<Object>> = Mutex::new(None);
impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            glib::gobject_ffi::g_object_unref(self.0.cast());
        }
    }
}
fn error(error: *mut glib::ffi::GError) -> anyhow::Error {
    if error.is_null() {
        return anyhow::anyhow!("LocalSearch returned no data");
    }
    let error: glib::Error = unsafe { from_glib_full(error) };
    anyhow::anyhow!(error.to_string())
}
pub fn literal(input: &str) -> String {
    format!(
        "\"{}\"",
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}
pub fn search(input: &str, cancel: &gio::Cancellable) -> Result<Vec<Item>> {
    if input.trim().is_empty() {
        return Ok(vec![]);
    }
    let needle = literal(&input.to_lowercase());
    let path_needle =
        literal(&glib::uri_escape_string(input.to_lowercase(), Some("/"), false).to_lowercase());
    let query = CString::new(format!(
        "SELECT DISTINCT ?url ?name WHERE {{ GRAPH tracker:FileSystem {{ ?f a nfo:FileDataObject ; nie:url ?url ; nfo:fileName ?name . FILTER (CONTAINS(LCASE(?name), {needle}) || CONTAINS(LCASE(?url), {path_needle})) }} }} ORDER BY DESC(CONTAINS(LCASE(?name), {needle})) STRLEN(?name) LIMIT 40"
    ))?;
    unsafe {
        // GObject registers process-global types with vtables in this library.
        // Unloading it after a query leaves dangling type callbacks inside GLib.
        static LIBRARY: OnceLock<std::result::Result<Library, String>> = OnceLock::new();
        let lib = LIBRARY
            .get_or_init(|| {
                Library::new("libtinysparql-3.0.so.0")
                    .or_else(|_| Library::new("libtracker-sparql-3.0.so.0"))
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| anyhow::anyhow!("LocalSearch client library is unavailable: {e}"))?;
        let connect: Connect = *lib.get(b"tracker_sparql_connection_bus_new\0")?;
        let run: Query = *lib.get(b"tracker_sparql_connection_query\0")?;
        let next: Next = *lib.get(b"tracker_sparql_cursor_next\0")?;
        let get: GetString = *lib.get(b"tracker_sparql_cursor_get_string\0")?;
        let mut err = ptr::null_mut();
        let cached = CONNECTION
            .lock()
            .ok()
            .and_then(|c| c.as_ref().map(Object::share));
        let connection = match cached {
            Some(connection) => connection,
            None => {
                let raw = connect(
                    c"org.freedesktop.LocalSearch3".as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                    &mut err,
                );
                if raw.is_null() {
                    return Err(error(err));
                }
                let connection = Object(raw);
                if let Ok(mut slot) = CONNECTION.lock() {
                    *slot = Some(connection.share());
                }
                connection
            }
        };
        let cursor = run(
            connection.0,
            query.as_ptr(),
            cancel.to_glib_none().0,
            &mut err,
        );
        if cursor.is_null() {
            if !cancel.is_cancelled()
                && let Ok(mut slot) = CONNECTION.lock()
            {
                // Reconnect next time in case LocalSearch restarted.
                slot.take();
            }
            return Err(error(err));
        }
        let cursor = Object(cursor);
        let mut items = Vec::new();
        while next(cursor.0, cancel.to_glib_none().0, &mut err) != 0 {
            let url = get(cursor.0, 0, ptr::null_mut());
            let name = get(cursor.0, 1, ptr::null_mut());
            if url.is_null() || name.is_null() {
                continue;
            }
            let url = CStr::from_ptr(url).to_string_lossy().into_owned();
            let file = gio::File::for_uri(&url);
            let Some(path) = file.path() else {
                continue;
            };
            // One stat answers both "still exists" and "is a folder".
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let name = CStr::from_ptr(name).to_string_lossy().into_owned();
            let icon = if meta.is_dir() {
                "folder".to_owned()
            } else {
                // Guessing from the name avoids reading file contents per result.
                let (content_type, _) = gio::content_type_guess(Some(&name), None);
                gio::content_type_get_icon(&content_type)
                    .to_string()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "text-x-generic".into())
            };
            items.push(Item {
                id: format!("file:{url}"),
                title: name,
                subtitle: path
                    .parent()
                    .map(|p| match p.strip_prefix(home()) {
                        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
                        Ok(rest) => format!("~/{}", rest.display()),
                        Err(_) => p.display().to_string(),
                    })
                    .unwrap_or_default(),
                icon,
                keywords: path.display().to_string(),
                kind: Kind::File(url),
            });
        }
        if !err.is_null() {
            return Err(error(err));
        }
        Ok(items)
    }
}
fn home() -> &'static std::path::Path {
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(glib::home_dir)
}
pub fn status() -> Result<String> {
    let output = std::process::Command::new("localsearch")
        .arg("status")
        .output()
        .context("LocalSearch is not installed")?;
    if !output.status.success() {
        bail!("LocalSearch is unavailable");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join("\n"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_sparql_literals() {
        assert_eq!(literal("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
    #[test]
    #[ignore = "requires a running GNOME LocalSearch session"]
    fn live_index() {
        let started = std::time::Instant::now();
        let items = search("png", &gio::Cancellable::new()).unwrap();
        eprintln!(
            "Live LocalSearch query: {:.2} ms",
            started.elapsed().as_secs_f64() * 1000.
        );
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.title.to_lowercase().contains("png")));
    }
}
