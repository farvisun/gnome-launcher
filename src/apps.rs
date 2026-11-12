use crate::model::{Item, Kind};
use gtk::{gio, prelude::*};

pub fn installed() -> Vec<Item> {
    let mut items: Vec<_> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .filter_map(|app| {
            let id = app.id()?.to_string();
            let icon = app
                .icon()
                .and_then(|i| i.to_string())
                .map(|i| i.to_string())
                .unwrap_or_else(|| "application-x-executable".into());
            let keywords = format!(
                "{} {} {}",
                app.name(),
                app.executable().display(),
                app.description().unwrap_or_default()
            );
            Some(Item {
                id: format!("app:{id}"),
                title: app.display_name().into(),
                subtitle: app
                    .description()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Application".into()),
                icon,
                keywords,
                kind: Kind::App(id),
            })
        })
        .collect();
    items.sort_by_key(|i| i.title.to_lowercase());
    items.dedup_by(|a, b| a.id == b.id);
    items
}
