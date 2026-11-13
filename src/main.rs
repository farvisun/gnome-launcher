mod apps;
mod calculator;
mod extensions;
mod files;
mod model;
mod store;
mod ui;

use anyhow::Result;
use gtk::gio;

pub const APP_ID: &str = "io.github.gnomelauncher.Launcher";

fn main() {
    if let Err(e) = run() {
        eprintln!("Lumen: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("Lumen {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help") => {
            println!(
                "Lumen — native GNOME launcher\n\n  gnome-launcher                 Toggle the palette\n  gnome-launcher --background    Stay resident without showing\n  gnome-launcher --settings      Open settings\n  gnome-launcher --quit          Stop the resident process\n  gnome-launcher doctor          Check runtime dependencies\n  gnome-launcher pack DIR OUTPUT Package an extension\n  gnome-launcher dev DIR         Register a developer extension\n  gnome-launcher search QUERY    Inspect LocalSearch results"
            );
            return Ok(());
        }
        Some("pack") => {
            anyhow::ensure!(args.len() == 4, "Usage: gnome-launcher pack DIR OUTPUT");
            extensions::package::pack(
                std::path::Path::new(&args[2]),
                std::path::Path::new(&args[3]),
            )?;
            println!("Packaged {}", args[3]);
            return Ok(());
        }
        Some("dev") => {
            anyhow::ensure!(args.len() == 3, "Usage: gnome-launcher dev DIR");
            let ext = extensions::package::load(std::path::Path::new(&args[2]))?;
            let mut state = store::Store::load();
            state.developers.insert(ext.directory);
            state.save()?;
            println!(
                "Registered {}. Reopen Settings to refresh extensions.",
                ext.manifest.name
            );
            return Ok(());
        }
        Some("search") => {
            let query = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: gnome-launcher search QUERY"))?;
            for item in files::search(query, &gio::Cancellable::new())? {
                println!("{}\t{}", item.title, item.subtitle);
            }
            return Ok(());
        }
        Some("doctor") => {
            println!("Lumen {}", env!("CARGO_PKG_VERSION"));
            println!("{} applications", apps::installed().len());
            println!("{}", files::status().unwrap_or_else(|e| e.to_string()));
            println!(
                "built-in extensions: {}",
                extensions::package::builtin_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "missing".into())
            );
            // systemd-run and notify-send ring timers; pkexec and fuser stop other users' ports.
            for binary in [
                "node",
                "gnome-extensions",
                "gsettings",
                "systemd-run",
                "notify-send",
                "pkexec",
                "fuser",
            ] {
                println!(
                    "{binary}: {}",
                    gio::Subprocess::newv(
                        &[std::ffi::OsStr::new("which"), std::ffi::OsStr::new(binary)],
                        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE
                    )
                    .and_then(|p| p.wait_check(None::<&gio::Cancellable>))
                    .is_ok()
                );
            }
            return Ok(());
        }
        Some(other) if !["--background", "--settings", "--quit"].contains(&other) => {
            anyhow::bail!("Unknown argument: {other}; use --help")
        }
        _ => {}
    }
    ui::run(&args);
    Ok(())
}
