use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Stage {
    root: PathBuf,
    extension: Extension,
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn prepare(source: &str) -> anyhow::Result<Stage> {
    let root = store::data_dir().join("staging").join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir_all(&root)?;
    let result = (|| {
        package::download(source, &root.join("package.tgz"))?;
        package::unpack(&root.join("package.tgz"), &root.join("extension"))
    })();
    match result {
        Ok(extension) => Ok(Stage { root, extension }),
        Err(e) => {
            let _ = fs::remove_dir_all(root);
            Err(e)
        }
    }
}
fn shortcut_settings() -> Option<gio::Settings> {
    let schema = "org.gnome.shell.extensions.lumen";
    let default = gio::SettingsSchemaSource::default()?;
    let local =
        glib::user_data_dir().join("gnome-shell/extensions/lumen@gnome-launcher.local/schemas");
    let source = gio::SettingsSchemaSource::from_directory(&local, Some(&default), false).ok();
    let schema = source
        .as_ref()
        .and_then(|s| s.lookup(schema, true))
        .or_else(|| default.lookup(schema, true))?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}
fn builtin_subtitle(ext: &Extension) -> String {
    let commands: Vec<&str> = ext
        .manifest
        .commands
        .iter()
        .map(|c| c.title.as_str())
        .collect();
    format!("{}\nCommands: {}", ext.manifest.description, commands.join(", "))
}
fn has_preferences(ext: &Extension) -> bool {
    !ext.manifest.preferences.is_empty()
        || ext.manifest.commands.iter().any(|c| !c.preferences.is_empty())
}
impl Ui {
    pub(super) fn open_settings(self: &Rc<Self>) {
        if let Some(window) = self
            .settings_window
            .borrow()
            .as_ref()
            .filter(|w| w.is_visible())
        {
            window.present();
            return;
        }
        self.cancel();
        self.state.borrow_mut().developers = Store::load().developers;
        self.refresh_extensions();
        let window = adw::PreferencesWindow::builder()
            .application(&self.app)
            .title("Lumen Settings")
            .default_width(680)
            .default_height(640)
            .search_enabled(true)
            .build();
        let general = adw::PreferencesPage::builder()
            .title("General")
            .icon_name("emblem-system-symbolic")
            .build();
        let behavior = adw::PreferencesGroup::builder()
            .title("Keyboard and preferences")
            .description("Super+Space opens Lumen. Super+Shift+Space switches keyboard layouts.")
            .build();
        let shortcut = adw::EntryRow::builder()
            .title("Launcher shortcut")
            .text("<Super>space")
            .build();
        if let Some(settings) = shortcut_settings() {
            shortcut.set_text(
                settings
                    .strv("toggle-lumen")
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("<Super>space"),
            );
            let apply = gtk::Button::with_label("Apply");
            apply.set_valign(gtk::Align::Center);
            shortcut.add_suffix(&apply);
            let weak = shortcut.downgrade();
            let settings = settings.clone();
            apply.connect_clicked(move |_| {
                if let Some(row) = weak.upgrade() {
                    if let Some((key, modifiers)) = gtk::accelerator_parse(row.text())
                        && !modifiers.is_empty()
                        && gtk::accelerator_valid(key, modifiers)
                    {
                        match settings.set_strv("toggle-lumen", [row.text().as_str()]) {
                            Ok(_) => row.remove_css_class("error"),
                            Err(_) => row.add_css_class("error"),
                        }
                        return;
                    }
                    row.add_css_class("error");
                    row.set_tooltip_text(Some(
                        "Use a valid accelerator such as <Super>space or <Control><Alt>space",
                    ));
                }
            });
        } else {
            shortcut.set_sensitive(false);
            shortcut.set_tooltip_text(Some(
                "Install the GNOME Shell integration to configure the shortcut",
            ));
        }
        behavior.add(&shortcut);
        let history = adw::ActionRow::builder()
            .use_markup(false)
            .title("Usage history")
            .subtitle("Used locally to rank suggestions; search text is never stored.")
            .build();
        let clear = gtk::Button::with_label("Clear history");
        clear.set_valign(gtk::Align::Center);
        history.add_suffix(&clear);
        let weak = Rc::downgrade(self);
        clear.connect_clicked(move |button| {
            if let Some(ui) = weak.upgrade() {
                ui.state.borrow_mut().usage.clear();
                ui.persist();
                button.set_label("Cleared");
                if ui.active.borrow().is_none() {
                    ui.render_search();
                }
            }
        });
        behavior.add(&history);
        let about = adw::ActionRow::builder()
            .use_markup(false)
            .title(concat!("Lumen ", env!("CARGO_PKG_VERSION")))
            .subtitle("A native command palette for GNOME · No telemetry or cloud account")
            .build();
        behavior.add(&about);
        general.add(&behavior);
        let results = adw::PreferencesGroup::builder()
            .title("Search results")
            .description("Choose which results lead when apps, files and commands all match.")
            .build();
        let orders = [ResultOrder::AppsFirst, ResultOrder::Relevance];
        let order = adw::ComboRow::builder()
            .title("Result order")
            .subtitle("Applications first keeps apps at the top of every search.")
            .model(&gtk::StringList::new(&["Applications first", "Best match first"]))
            .build();
        let current = self.state.borrow().result_order;
        order.set_selected(orders.iter().position(|o| *o == current).unwrap_or(0) as u32);
        let weak = Rc::downgrade(self);
        order.connect_selected_notify(move |row| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let chosen = orders
                .get(row.selected() as usize)
                .copied()
                .unwrap_or_default();
            if ui.state.borrow().result_order == chosen {
                return;
            }
            ui.state.borrow_mut().result_order = chosen;
            ui.persist();
            if ui.active.borrow().is_none() {
                ui.render_search();
            }
        });
        results.add(&order);
        general.add(&results);
        window.add(&general);

        let files_page = adw::PreferencesPage::builder()
            .title("Files")
            .icon_name("folder-symbolic")
            .build();
        let indexing = adw::PreferencesGroup::builder().title("GNOME file index").description("Lumen searches file and folder names using LocalSearch. Indexed folders and exclusions are shared with GNOME Files.").build();
        let index_status = adw::ActionRow::builder()
            .use_markup(false)
            .title("Index status")
            .subtitle("Checking LocalSearch…")
            .build();
        indexing.add(&index_status);
        let folders = adw::ActionRow::builder()
            .use_markup(false)
            .title("Indexed folders")
            .subtitle("Choose the folders GNOME indexes in Search settings.")
            .build();
        let open = gtk::Button::with_label("Open Search Settings");
        open.set_valign(gtk::Align::Center);
        folders.add_suffix(&open);
        let row = folders.clone();
        open.connect_clicked(move |_| {
            if let Err(e) = gio::Subprocess::newv(
                &[
                    std::ffi::OsStr::new("gnome-control-center"),
                    std::ffi::OsStr::new("search"),
                ],
                gio::SubprocessFlags::NONE,
            ) {
                row.set_subtitle(&e.to_string());
            }
        });
        indexing.add(&folders);
        files_page.add(&indexing);
        window.add(&files_page);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(files::status().unwrap_or_else(|e| e.to_string()));
        });
        let weak_row = index_status.downgrade();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(row) = weak_row.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(text) => {
                    row.set_subtitle(&text);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            }
        });

        let extensions_page = adw::PreferencesPage::builder()
            .title("Extensions")
            .name("extensions")
            .icon_name("application-x-addon-symbolic")
            .build();
        self.settings_rows.borrow_mut().clear();
        extensions_page.add(&self.builtin_group());
        let install = adw::PreferencesGroup::builder().title("Install an extension").description("Choose a .lumen-extension package or paste its HTTPS URL. Review its source and capabilities before enabling it.").build();
        let source = adw::EntryRow::builder()
            .title("Package path or HTTPS URL")
            .build();
        let choose = gtk::Button::from_icon_name("folder-open-symbolic");
        choose.set_valign(gtk::Align::Center);
        choose.set_tooltip_text(Some("Choose extension package"));
        source.add_suffix(&choose);
        let weak_source = source.downgrade();
        let parent = window.downgrade();
        choose.connect_clicked(move |_| {
            let source = weak_source.clone();
            let parent = parent.clone();
            glib::spawn_future_local(async move {
                let dialog = gtk::FileDialog::builder()
                    .title("Choose extension package")
                    .build();
                if let Ok(file) = dialog.open_future(parent.upgrade().as_ref()).await
                    && let (Some(row), Some(path)) = (source.upgrade(), file.path())
                {
                    row.set_text(&path.display().to_string());
                }
            });
        });
        let inspect = gtk::Button::with_label("Review");
        inspect.set_valign(gtk::Align::Center);
        source.add_suffix(&inspect);
        install.add(&source);
        let install_status = adw::ActionRow::builder()
            .use_markup(false)
            .title("Ready to install")
            .subtitle("Packages are self-contained; installation scripts are never run.")
            .build();
        install.add(&install_status);
        let weak = Rc::downgrade(self);
        let row = install_status.clone();
        let parent = window.downgrade();
        let source_weak = source.downgrade();
        inspect.connect_clicked(move |button| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Some(source) = source_weak.upgrade() else {
                return;
            };
            let Some(parent) = parent.upgrade() else {
                return;
            };
            let location = source.text().trim().to_owned();
            if location.is_empty() {
                row.set_subtitle("Choose a package or enter an HTTPS URL.");
                return;
            }
            button.set_sensitive(false);
            row.set_title("Inspecting package…");
            row.set_subtitle("Downloading and validating the extension");
            let (tx, rx) = mpsc::channel();
            let request_source = location.clone();
            std::thread::spawn(move || {
                let _ = tx.send(prepare(&request_source).map_err(|e| format!("{e:#}")));
            });
            let button = button.downgrade();
            let row = row.downgrade();
            let weak = Rc::downgrade(&ui);
            let parent = parent.downgrade();
            glib::timeout_add_local(Duration::from_millis(40), move || {
                match rx.try_recv() {
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Ok(Ok(stage)) => {
                        if let (Some(ui), Some(row), Some(parent)) =
                            (weak.upgrade(), row.upgrade(), parent.upgrade())
                        {
                            ui.review_package(stage, &location, &parent, &row);
                        }
                    }
                    Ok(Err(e)) => {
                        if let Some(row) = row.upgrade() {
                            row.set_title("Could not install extension");
                            row.set_subtitle(&e);
                        }
                    }
                    Err(_) => {
                        if let Some(row) = row.upgrade() {
                            row.set_subtitle("Package inspection stopped unexpectedly");
                        }
                    }
                }
                if let Some(button) = button.upgrade() {
                    button.set_sensitive(true);
                }
                glib::ControlFlow::Break
            });
        });
        extensions_page.add(&install);

        let installed = adw::PreferencesGroup::builder()
            .title("Installed extensions")
            .build();
        if self.extensions.borrow().iter().all(|e| e.builtin) {
            installed.add(
                &adw::ActionRow::builder()
                    .use_markup(false)
                    .title("No extensions installed yet")
                    .subtitle("Install a package above or load a developer project below.")
                    .build(),
            );
        }
        for ext in self.extensions.borrow().iter().filter(|e| !e.builtin) {
            let row = adw::ActionRow::builder()
                .use_markup(false)
                .title(&ext.manifest.name)
                .subtitle(format!(
                    "{} · {}",
                    ext.manifest.version, ext.manifest.description
                ))
                .build();
            row.set_tooltip_text(Some(&ext.directory.display().to_string()));
            let toggle = gtk::Switch::builder()
                .active(!self.state.borrow().disabled.contains(&ext.manifest.id))
                .valign(gtk::Align::Center)
                .build();
            toggle.set_tooltip_text(Some("Enable extension"));
            row.add_suffix(&toggle);
            let id = ext.manifest.id.clone();
            let weak = Rc::downgrade(self);
            toggle.connect_active_notify(move |toggle| {
                if let Some(ui) = weak.upgrade() {
                    {
                        let mut state = ui.state.borrow_mut();
                        if toggle.is_active() {
                            state.disabled.remove(&id);
                        } else {
                            state.disabled.insert(id.clone());
                        }
                    }
                    ui.cancel();
                    ui.runtime.stop(&id);
                    ui.persist();
                    ui.refresh_extensions();
                }
            });
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.set_valign(gtk::Align::Center);
            remove.set_tooltip_text(Some("Remove extension"));
            row.add_suffix(&remove);
            let ext = ext.clone();
            let weak = Rc::downgrade(self);
            let row_weak = row.downgrade();
            let parent = window.downgrade();
            remove.connect_clicked(move |_| {
                let (Some(ui), Some(parent)) = (weak.upgrade(), parent.upgrade()) else { return; };
                let dialog = adw::AlertDialog::builder().heading(format!("Remove {}?", ext.manifest.name)).body("The extension will be disabled and removed. Its saved extension data will be kept.").build();
                dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]); dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive); dialog.set_close_response("cancel");
                let ext = ext.clone(); let weak = Rc::downgrade(&ui); let row = row_weak.clone();
                dialog.connect_response(None, move |_, response| {
                    if response != "remove" { return; }
                    if let Some(ui) = weak.upgrade() {
                        ui.cancel(); ui.runtime.stop(&ext.manifest.id);
                        let developer = ui.state.borrow().developers.contains(&ext.directory);
                        let result = if developer { ui.state.borrow_mut().developers.remove(&ext.directory); Ok(()) } else {
                            let target = store::data_dir().join("extensions").join(&ext.manifest.id);
                            if target == ext.directory { fs::remove_dir_all(&target) } else { Err(std::io::Error::other("Refusing to remove a directory outside extension storage")) }
                        };
                        if let Err(e) = result { if let Some(row) = row.upgrade() { row.set_subtitle(&e.to_string()); } return; }
                        ui.state.borrow_mut().disabled.remove(&ext.manifest.id); ui.persist(); ui.refresh_extensions(); if let Some(row) = row.upgrade() { row.set_visible(false); }
                    }
                }); dialog.present(Some(&parent));
            });
            installed.add(&row);
        }
        extensions_page.add(&installed);
        let configurable: Vec<Extension> = self
            .extensions
            .borrow()
            .iter()
            .filter(|e| !e.builtin && has_preferences(e))
            .cloned()
            .collect();
        if !configurable.is_empty() {
            let group = adw::PreferencesGroup::builder().title("Extension preferences").build();
            for ext in configurable {
                let row = adw::ExpanderRow::builder()
                    .use_markup(false)
                    .title(&ext.manifest.name)
                    .build();
                for pref in self.preference_rows(&ext) {
                    row.add_row(&pref);
                }
                self.settings_rows.borrow_mut().push((ext.manifest.id.clone(), row.clone()));
                group.add(&row);
            }
            extensions_page.add(&group);
        }
        let developer = adw::PreferencesGroup::builder().title("Develop extensions").description("Load a local project containing extension.json. Developer folders are trusted local programs and are never deleted by Lumen.").build();
        let folder = adw::EntryRow::builder()
            .title("Developer project folder")
            .build();
        let load = gtk::Button::with_label("Load");
        load.set_valign(gtk::Align::Center);
        folder.add_suffix(&load);
        let weak_folder = folder.downgrade();
        let weak = Rc::downgrade(self);
        load.connect_clicked(move |button| {
            if let (Some(ui), Some(folder)) = (weak.upgrade(), weak_folder.upgrade()) {
                match package::load(Path::new(folder.text().as_str())) {
                    Ok(ext) => {
                        ui.state.borrow_mut().developers.insert(ext.directory);
                        ui.persist();
                        ui.refresh_extensions();
                        button.set_label("Loaded");
                        folder.remove_css_class("error");
                    }
                    Err(e) => {
                        folder.add_css_class("error");
                        folder.set_tooltip_text(Some(&format!("{e:#}")));
                    }
                }
            }
        });
        developer.add(&folder);
        extensions_page.add(&developer);
        window.add(&extensions_page);
        *self.settings_window.borrow_mut() = Some(window.clone());
        window.present();
    }
    /// Opens Settings at an extension's preferences.
    pub(super) fn open_settings_at(self: &Rc<Self>, id: &str) {
        self.open_settings();
        let Some(window) = self.settings_window.borrow().clone() else { return };
        window.set_visible_page_name("extensions");
        if let Some((_, row)) = self.settings_rows.borrow().iter().find(|(e, _)| e == id) {
            row.set_expanded(true);
            row.grab_focus();
        }
    }
    /// One row per manifest preference, saved as soon as it changes. Command
    /// preferences follow a heading row with the command's title.
    fn preference_rows(self: &Rc<Self>, ext: &Extension) -> Vec<gtk::Widget> {
        let mut rows: Vec<gtk::Widget> = Vec::new();
        let id = &ext.manifest.id;
        let scopes = std::iter::once((id.clone(), None, &ext.manifest.preferences)).chain(
            ext.manifest
                .commands
                .iter()
                .filter(|c| !c.preferences.is_empty())
                .map(|c| (format!("{id}/{}", c.id), Some(c.title.clone()), &c.preferences)),
        );
        for (key, heading, preferences) in scopes {
            if preferences.is_empty() {
                continue;
            }
            if let Some(heading) = heading {
                let row = adw::ActionRow::builder().use_markup(false).title(&heading).activatable(false).build();
                row.add_css_class("preference-heading");
                rows.push(row.upcast());
            }
            for pref in preferences {
                let current = self
                    .state
                    .borrow()
                    .preferences
                    .get(&key)
                    .and_then(|m| m.get(&pref.name))
                    .cloned()
                    .unwrap_or_else(|| pref.fallback());
                let weak = Rc::downgrade(self);
                let (key, name, ext_id) = (key.clone(), pref.name.clone(), id.clone());
                let save = move |value: Value| {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.state
                        .borrow_mut()
                        .preferences
                        .entry(key.clone())
                        .or_default()
                        .insert(name.clone(), value);
                    ui.persist();
                    ui.runtime.stop(&ext_id);
                };
                let title = if pref.required { format!("{} (required)", pref.title) } else { pref.title.clone() };
                let row: gtk::Widget = match pref.kind.as_str() {
                    "checkbox" => {
                        let row = adw::SwitchRow::builder()
                            .use_markup(false)
                            .title(if pref.label.is_empty() { &title } else { &pref.label })
                            .subtitle(if pref.label.is_empty() { &pref.description } else { &title })
                            .active(current.as_bool().unwrap_or(false))
                            .build();
                        row.connect_active_notify(move |row| save(json!(row.is_active())));
                        row.upcast()
                    }
                    "dropdown" => {
                        let titles: Vec<&str> = pref.data.iter().map(|c| c.title.as_str()).collect();
                        let values: Vec<String> = pref.data.iter().map(|c| c.value.clone()).collect();
                        let row = adw::ComboRow::builder()
                            .use_markup(false)
                            .title(&title)
                            .subtitle(&pref.description)
                            .model(&gtk::StringList::new(&titles))
                            .build();
                        if let Some(at) = values.iter().position(|v| current == **v) {
                            row.set_selected(at as u32);
                        }
                        row.connect_selected_notify(move |row| {
                            if let Some(value) = values.get(row.selected() as usize) {
                                save(json!(value));
                            }
                        });
                        row.upcast()
                    }
                    kind => {
                        let row: adw::EntryRow = if kind == "password" {
                            adw::PasswordEntryRow::builder().title(&title).build().upcast()
                        } else {
                            adw::EntryRow::builder().title(&title).build()
                        };
                        row.set_text(current.as_str().unwrap_or(""));
                        row.set_tooltip_text(Some(&pref.description).filter(|d| !d.is_empty()).map(String::as_str));
                        row.set_show_apply_button(true);
                        row.connect_apply(move |row| save(json!(row.text().as_str())));
                        row.upcast()
                    }
                };
                rows.push(row);
            }
        }
        rows
    }
    /// Extensions that ship with Lumen. They start off; turning one on
    /// only adds its commands to the palette.
    fn builtin_group(self: &Rc<Self>) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Built-in extensions")
            .description("Extra commands that come with Lumen. Turn on the ones you want.")
            .build();
        let builtins: Vec<Extension> = self
            .extensions
            .borrow()
            .iter()
            .filter(|e| e.builtin)
            .cloned()
            .collect();
        if builtins.is_empty() {
            group.add(
                &adw::ActionRow::builder()
                    .use_markup(false)
                    .title("Built-in extensions are missing")
                    .subtitle("Reinstall Lumen to restore them.")
                    .build(),
            );
        }
        for ext in builtins {
            let enabled = self.state.borrow().is_enabled(&ext);
            let subtitle = builtin_subtitle(&ext);
            let id = ext.manifest.id.clone();
            let weak = Rc::downgrade(self);
            let set = move |active: bool| {
                let Some(ui) = weak.upgrade() else { return };
                {
                    let mut state = ui.state.borrow_mut();
                    if active == state.enabled_builtins.contains(&id) {
                        return;
                    }
                    if active {
                        state.enabled_builtins.insert(id.clone());
                    } else {
                        state.enabled_builtins.remove(&id);
                    }
                }
                ui.cancel();
                ui.runtime.stop(&id);
                ui.persist();
                ui.refresh_extensions();
            };
            let indicator = match ext.manifest.id.as_str() {
                "lumen.timers" => Some((
                    "show-timer-indicator",
                    "Show countdown in the top bar",
                    "Shows the next timer to finish while timers are running.",
                    "Install the GNOME Shell integration to show timers in the top bar.",
                )),
                "lumen.weather" => Some((
                    "show-weather-indicator",
                    "Show weather in the top bar",
                    "The current conditions, refreshed every 30 minutes, with the forecast in its menu.",
                    "Install the GNOME Shell integration to show the weather in the top bar.",
                )),
                _ => None,
            };
            if indicator.is_some() || has_preferences(&ext) {
                let row = adw::ExpanderRow::builder()
                    .use_markup(false)
                    .title(&ext.manifest.name)
                    .subtitle(&subtitle)
                    .show_enable_switch(true)
                    .enable_expansion(enabled)
                    .build();
                row.connect_enable_expansion_notify(move |row| set(row.enables_expansion()));
                if let Some((key, title, about, missing)) = indicator {
                    let switch = adw::SwitchRow::builder().title(title).subtitle(about).build();
                    match shortcut_settings()
                        .filter(|s| s.settings_schema().is_some_and(|k| k.has_key(key)))
                    {
                        // The binding holds its own reference to the settings object.
                        Some(settings) => settings.bind(key, &switch, "active").build(),
                        None => {
                            switch.set_sensitive(false);
                            switch.set_subtitle(missing);
                        }
                    }
                    row.add_row(&switch);
                }
                for pref in self.preference_rows(&ext) {
                    row.add_row(&pref);
                }
                self.settings_rows.borrow_mut().push((ext.manifest.id.clone(), row.clone()));
                group.add(&row);
            } else {
                let row = adw::SwitchRow::builder()
                    .use_markup(false)
                    .title(&ext.manifest.name)
                    .subtitle(&subtitle)
                    .active(enabled)
                    .build();
                row.connect_active_notify(move |row| set(row.is_active()));
                group.add(&row);
            }
        }
        group
    }
    fn review_package(
        self: &Rc<Self>,
        stage: Stage,
        source: &str,
        parent: &adw::PreferencesWindow,
        status: &adw::ActionRow,
    ) {
        let manifest = &stage.extension.manifest;
        let target = store::data_dir().join("extensions").join(&manifest.id);
        let replacing = target.exists();
        let heading = format!(
            "{} {}?",
            if replacing { "Replace" } else { "Install" },
            manifest.name
        );
        let capabilities = if manifest.capabilities.is_empty() {
            "None declared".into()
        } else {
            manifest.capabilities.join(", ")
        };
        let dialog = adw::AlertDialog::builder().heading(heading).body(format!("{} · version {}\n\nSource: {}\nCapabilities: {}\n\nExtensions run as your user and can access your files and network. Install only programs you trust.", manifest.id, manifest.version, source, capabilities)).build();
        dialog.add_responses(&[
            ("cancel", "Cancel"),
            (
                "install",
                if replacing {
                    "Replace & Enable"
                } else {
                    "Install & Enable"
                },
            ),
        ]);
        dialog.set_response_appearance("install", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let stage = Rc::new(stage);
        let weak = Rc::downgrade(self);
        let row = status.downgrade();
        dialog.connect_response(None, move |_, response| {
            if response != "install" { if let Some(row) = row.upgrade() { row.set_title("Installation cancelled"); row.set_subtitle("No extension was installed."); } return; }
            if let Some(ui) = weak.upgrade() {
                ui.cancel(); ui.runtime.stop(&stage.extension.manifest.id);
                match package::install(&stage.extension.directory, &store::data_dir().join("extensions"), replacing) {
                    Ok(()) => { ui.state.borrow_mut().disabled.remove(&stage.extension.manifest.id); ui.persist(); ui.refresh_extensions(); if let Some(row) = row.upgrade() { row.set_title("Extension installed"); row.set_subtitle("Its commands are available in the palette. Reopen Settings to manage it."); } }
                    Err(e) => if let Some(row) = row.upgrade() { row.set_title("Installation failed"); row.set_subtitle(&format!("{e:#}")); },
                }
            }
        });
        dialog.present(Some(parent));
    }
}
