mod grid;
mod rich;
mod settings;
pub use rich::COLORS;

use crate::{
    apps, calculator,
    extensions::{
        package::{self, Extension, Mode},
        runtime::{self, Runtime},
    },
    files,
    model::{self, Item, Kind},
    store::{self, ResultOrder, Store},
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const PLACEHOLDER: &str = "Search apps, files, and commands…";
const SUGGESTION_LIMIT: usize = 12;
const RESULT_LIMIT: usize = 40;
const SECTION_LIMIT: usize = 8;

type MenuAction = Box<dyn Fn(&Rc<Ui>)>;

/// A searchable item with its text lowercased once, when the catalog is built.
struct Candidate {
    item: Item,
    title: String,
    keywords: String,
}
impl Candidate {
    fn new(item: Item) -> Self {
        Self {
            title: item.title.to_lowercase(),
            keywords: item.keywords.to_lowercase(),
            item,
        }
    }
}

pub struct Ui {
    app: adw::Application,
    window: adw::ApplicationWindow,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    /// Side pane for an extension list's `detail`, beside the results.
    side: gtk::Box,
    side_scroll: gtk::ScrolledWindow,
    stack: gtk::Stack,
    detail: gtk::Box,
    scope: gtk::Button,
    status: gtk::Label,
    primary_hint: gtk::Box,
    primary_label: gtk::Label,
    action_button: gtk::MenuButton,
    pub(super) state: RefCell<Store>,
    apps: RefCell<Vec<Item>>,
    /// Kept alive for the life of the window: GIO destroys the monitor, and stops
    /// reporting installs and removals, as soon as its last reference is dropped.
    app_monitor: gio::AppInfoMonitor,
    /// Pending app list reload; package installs change many files in a burst.
    app_reload: RefCell<Option<glib::SourceId>>,
    /// Apps, extension commands and built-ins; rebuilt only when one of them changes.
    catalog: RefCell<Option<Rc<Vec<Candidate>>>>,
    pub(super) extensions: RefCell<Vec<Extension>>,
    pub(super) runtime: Arc<Runtime>,
    results: RefCell<Vec<Item>>,
    /// What the visible rows were built from, so identical updates skip a rebuild:
    /// sections, favourites, and each extension row's raw JSON (null elsewhere).
    rendered: RefCell<(Vec<String>, Vec<bool>, Vec<Value>)>,
    /// Pending timed refresh of an extension list that asked for one.
    refresh: RefCell<Option<glib::SourceId>>,
    file_results: RefCell<Vec<Candidate>>,
    generation: Cell<u64>,
    file_cancel: RefCell<Option<gio::Cancellable>>,
    command_cancel: RefCell<Option<Arc<AtomicBool>>>,
    debounce: RefCell<Option<glib::SourceId>>,
    suppress_search: Cell<bool>,
    active: RefCell<Option<(Extension, String)>>,
    view: RefCell<Value>,
    /// Views an extension pushed over, with their search text; Esc pops them.
    views: RefCell<Vec<(Value, String)>>,
    /// The request generation that last pushed, so its later updates replace
    /// the pushed view instead of stacking more copies.
    pushed: Cell<Option<u64>>,
    /// Arguments and launch context the active command was opened with.
    launch: RefCell<Value>,
    /// The active command runs without a view and closes Lumen when done.
    no_view: Cell<bool>,
    /// Set by a failure toast, so a no-view command keeps the window open.
    failed: Cell<bool>,
    /// A toast replaced the status line during the current request.
    toasted: Cell<bool>,
    /// HUD text to show, and whether to close, once the current request ends.
    pending_hud: RefCell<Option<String>>,
    pending_close: Cell<bool>,
    /// A dialog of ours (the file chooser) has focus; don't close on blur.
    modal: Cell<bool>,
    spinner: gtk::Spinner,
    /// Inline argument fields for the selected command, beside the search.
    arguments: gtk::Box,
    argument_fields: RefCell<Vec<(String, gtk::Widget, bool, Vec<String>)>>,
    argument_owner: RefCell<String>,
    grid_scroll: gtk::ScrolledWindow,
    grid: RefCell<Option<grid::Grid>>,
    grid_selected: Cell<usize>,
    /// The selected item's ID when a `selection` event was last sent.
    selected_id: RefCell<String>,
    select_debounce: RefCell<Option<glib::SourceId>>,
    form: RefCell<Vec<(String, gtk::Widget, bool, Vec<String>)>>,
    pub(super) settings_window: RefCell<Option<adw::PreferencesWindow>>,
    /// Each extension's expander in Settings, to open one from a command.
    pub(super) settings_rows: RefCell<Vec<(String, adw::ExpanderRow)>>,
    /// Running close fade; reopening cancels it instead of waiting.
    closing: RefCell<Option<adw::TimedAnimation>>,
    _hold: gio::ApplicationHoldGuard,
}

pub fn run(args: &[String]) {
    let app = adw::Application::builder()
        .application_id(crate::APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Rc::default();
    let holder = ui.clone();
    app.connect_startup(move |app| {
        let ui = Ui::new(app);
        *holder.borrow_mut() = Some(ui.clone());
        for name in ["toggle", "show", "settings", "quit"] {
            let action = gio::SimpleAction::new(name, None);
            let ui = Rc::downgrade(&ui);
            action.connect_activate(move |action, _| {
                if let Some(ui) = ui.upgrade() {
                    match action.name().as_str() {
                        "toggle" => ui.toggle(),
                        "show" => ui.show(),
                        "settings" => ui.open_settings(),
                        "quit" => ui.app.quit(),
                        _ => {}
                    }
                }
            });
            app.add_action(&action);
        }
        // "<extension-id>/<command-id>": the top-bar timer menu opens a command directly.
        let action = gio::SimpleAction::new("run-command", Some(glib::VariantTy::STRING));
        let weak = Rc::downgrade(&ui);
        action.connect_activate(move |_, parameter| {
            let Some(ui) = weak.upgrade() else { return };
            // An optional `#<json>` suffix is the command's launch context.
            let Some(target) = parameter.and_then(|p| p.str()) else { return };
            let (target, context) = match target.split_once('#') {
                Some((target, json)) => (target, serde_json::from_str(json).unwrap_or(Value::Null)),
                None => (target, Value::Null),
            };
            let Some((extension, command)) = target.split_once('/') else {
                return;
            };
            ui.show();
            if let Err(e) = ui.open_command(extension, command, json!({"launch_context": context})) {
                ui.status.set_label(&format!("{e:#}"));
            }
        });
        app.add_action(&action);
    });
    let holder = ui.clone();
    app.connect_command_line(move |_, command| {
        if let Some(ui) = holder.borrow().as_ref() {
            let args = command.arguments();
            if args.iter().any(|a| a == "--quit") {
                ui.app.quit();
            } else if args.iter().any(|a| a == "--settings") {
                ui.open_settings();
            } else if !args.iter().any(|a| a == "--background") {
                ui.toggle();
            }
        }
        glib::ExitCode::SUCCESS
    });
    let holder = ui.clone();
    app.connect_shutdown(move |_| {
        if let Some(ui) = holder.borrow().as_ref() {
            ui.cancel();
            ui.runtime.shutdown();
        }
    });
    app.run_with_args(args);
}

/// A key hint such as `[↵] Open`, drawn as a small keycap and a dim label.
fn key_hint(keys: &[&str], label: &str) -> (gtk::Box, gtk::Label) {
    let hint = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    hint.add_css_class("key-hint");
    hint.set_valign(gtk::Align::Center);
    for key in keys {
        let cap = gtk::Label::new(Some(key));
        cap.add_css_class("keycap");
        cap.set_valign(gtk::Align::Center);
        hint.append(&cap);
    }
    let text = gtk::Label::new(Some(label));
    text.set_margin_start(2);
    hint.append(&text);
    (hint, text)
}

impl Ui {
    fn new(app: &adw::Application) -> Rc<Self> {
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("../data/style.css"));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("A desktop display is required"),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Lumen Launcher")
            .default_width(720)
            .default_height(480)
            .resizable(false)
            // Client-side decorations give the palette a real compositor shadow
            // and transparent rounded corners; the shadow stays outside both the
            // input region and the frame rect the Shell centers.
            .decorated(true)
            .build();
        window.add_css_class("palette");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.add_css_class("search-bar");
        let scope = gtk::Button::builder()
            .visible(false)
            .valign(gtk::Align::Center)
            .tooltip_text("Back (Esc)")
            .build();
        scope.add_css_class("scope");
        top.append(&scope);
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(PLACEHOLDER)
            .hexpand(true)
            .build();
        entry.add_css_class("palette-search");
        entry.set_tooltip_text(Some("Search apps, files, commands, or enter a calculation"));
        top.append(&entry);
        let arguments = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        arguments.add_css_class("arguments");
        arguments.set_visible(false);
        arguments.set_valign(gtk::Align::Center);
        top.append(&arguments);
        let settings_button = gtk::Button::from_icon_name("emblem-system-symbolic");
        settings_button.add_css_class("flat");
        settings_button.add_css_class("circular");
        settings_button.set_valign(gtk::Align::Center);
        settings_button.set_tooltip_text(Some("Settings (Ctrl+,)"));
        top.append(&settings_button);
        root.append(&top);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .activate_on_single_click(true)
            .build();
        list.add_css_class("results");
        list.set_header_func(|row, previous| {
            let name = row.widget_name();
            if let Some(section) = name.strip_prefix("section:")
                && previous.is_none_or(|before| before.widget_name() != name)
            {
                let label = gtk::Label::builder().label(section).xalign(0.).build();
                label.add_css_class("section-label");
                row.set_header(Some(&label));
            } else {
                row.set_header(None::<&gtk::Widget>);
            }
        });
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let detail = gtk::Box::new(gtk::Orientation::Vertical, 12);
        detail.add_css_class("detail");
        let detail_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&detail)
            .build();
        let side = gtk::Box::new(gtk::Orientation::Vertical, 10);
        side.add_css_class("side-pane");
        let side_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .width_request(280)
            .visible(false)
            .child(&side)
            .build();
        let results_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        scroll.set_hexpand(true);
        results_area.append(&scroll);
        results_area.append(&side_scroll);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&results_area, Some("results"));
        stack.add_named(&detail_scroll, Some("detail"));
        let grid_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        stack.add_named(&grid_scroll, Some("grid"));
        root.append(&stack);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        footer.add_css_class("footer");
        let brand = gtk::Image::from_icon_name("io.github.gnomelauncher.Launcher");
        brand.set_pixel_size(16);
        brand.add_css_class("brand");
        footer.append(&brand);
        let spinner = gtk::Spinner::builder().visible(false).build();
        footer.append(&spinner);
        let status = gtk::Label::builder()
            .label("")
            .hexpand(true)
            .xalign(0.)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        status.add_css_class("status");
        footer.append(&status);
        let (primary_hint, primary_label) = key_hint(&["↵"], "Open");
        // The divider belongs to the hint so both hide when Enter does nothing.
        let divider = gtk::Separator::new(gtk::Orientation::Vertical);
        divider.set_margin_start(10);
        primary_hint.append(&divider);
        footer.append(&primary_hint);
        let (actions_hint, _) = key_hint(&["Ctrl", "K"], "Actions");
        let action_button = gtk::MenuButton::builder()
            .child(&actions_hint)
            .always_show_arrow(false)
            .tooltip_text("Actions for the selected result")
            .build();
        action_button.add_css_class("flat");
        action_button.add_css_class("actions-button");
        footer.append(&action_button);
        root.append(&footer);
        window.set_content(Some(&root));
        let state = Store::load();
        let extensions = package::discover(&state);
        let ui = Rc::new(Self {
            app: app.clone(),
            window,
            entry,
            list,
            scroll,
            side,
            side_scroll,
            stack,
            detail,
            scope,
            status,
            primary_hint,
            primary_label,
            action_button,
            apps: RefCell::new(apps::installed()),
            app_monitor: gio::AppInfoMonitor::get(),
            app_reload: RefCell::default(),
            catalog: RefCell::default(),
            extensions: RefCell::new(extensions),
            state: RefCell::new(state),
            runtime: Arc::default(),
            results: RefCell::default(),
            rendered: RefCell::default(),
            refresh: RefCell::default(),
            file_results: RefCell::default(),
            generation: Cell::new(0),
            file_cancel: RefCell::default(),
            command_cancel: RefCell::default(),
            debounce: RefCell::default(),
            suppress_search: Cell::new(false),
            active: RefCell::default(),
            view: RefCell::new(Value::Null),
            views: RefCell::default(),
            pushed: Cell::new(None),
            launch: RefCell::new(json!({})),
            no_view: Cell::new(false),
            failed: Cell::new(false),
            toasted: Cell::new(false),
            pending_hud: RefCell::default(),
            pending_close: Cell::new(false),
            modal: Cell::new(false),
            spinner,
            arguments,
            argument_fields: RefCell::default(),
            argument_owner: RefCell::default(),
            grid_scroll,
            grid: RefCell::default(),
            grid_selected: Cell::new(0),
            selected_id: RefCell::default(),
            select_debounce: RefCell::default(),
            form: RefCell::default(),
            settings_window: RefCell::default(),
            settings_rows: RefCell::default(),
            closing: RefCell::default(),
            _hold: app.hold(),
        });
        let weak = Rc::downgrade(&ui);
        settings_button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.open_settings();
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.scope.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.go_back();
                ui.entry.grab_focus();
            }
        });
        // Build the menu when it opens instead of on every selection change.
        let weak = Rc::downgrade(&ui);
        ui.action_button.set_create_popup_func(move |button| {
            if let Some(ui) = weak.upgrade() {
                button.set_popover(Some(&ui.build_actions()));
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.entry.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade()
                && !ui.suppress_search.get()
            {
                ui.search_changed();
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.entry.connect_activate(move |_| {
            if let Some(ui) = weak.upgrade()
                && let Some(index) = ui.selected_index()
            {
                ui.activate(index);
            }
        });
        ui.entry.set_search_delay(0);
        let weak = Rc::downgrade(&ui);
        ui.list.connect_row_activated(move |_, row| {
            if let Some(ui) = weak.upgrade() {
                ui.activate(row.index() as usize);
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.list.connect_row_selected(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.update_hints();
                ui.update_side();
                ui.update_arguments();
                ui.selection_changed();
            }
        });
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&ui);
        key.connect_key_pressed(move |_, key, _, mods| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let page = ui.stack.visible_child_name();
            let results = page.as_deref() == Some("results");
            let grid = page.as_deref() == Some("grid");
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            match key {
                gdk::Key::Escape => {
                    if ui.action_button.is_active() {
                        ui.action_button.popdown();
                    } else if ui.active.borrow().is_some() {
                        ui.go_back();
                    } else if !ui.entry.text().is_empty() {
                        ui.entry.set_text("");
                    } else {
                        ui.hide();
                    }
                }
                gdk::Key::BackSpace
                    if ui.active.borrow().is_some()
                        && ui.entry.text().is_empty()
                        && ui.entry.has_focus() =>
                {
                    ui.go_back()
                }
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab
                    if ui.arguments.is_visible() && !ui.argument_fields.borrow().is_empty() =>
                {
                    ui.cycle_arguments(if shift { -1 } else { 1 });
                }
                gdk::Key::Right | gdk::Key::KP_Right if grid => ui.move_grid(1, 0),
                gdk::Key::Left | gdk::Key::KP_Left if grid => ui.move_grid(-1, 0),
                gdk::Key::Down | gdk::Key::KP_Down if grid => ui.move_grid(0, 1),
                gdk::Key::Up | gdk::Key::KP_Up if grid => ui.move_grid(0, -1),
                gdk::Key::Down | gdk::Key::KP_Down if results => ui.move_selection(1),
                gdk::Key::Up | gdk::Key::KP_Up if results => ui.move_selection(-1),
                gdk::Key::n if ctrl && results => ui.move_selection(1),
                gdk::Key::p if ctrl && results => ui.move_selection(-1),
                gdk::Key::Page_Down if results => ui.move_selection(5),
                gdk::Key::Page_Up if results => ui.move_selection(-5),
                gdk::Key::k if ctrl => ui.action_button.popup(),
                gdk::Key::comma if ctrl => ui.open_settings(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        ui.window.add_controller(key);
        let weak = Rc::downgrade(&ui);
        ui.window.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.hide();
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(&ui);
        ui.window.connect_is_active_notify(move |window| {
            if !window.is_active() && window.is_visible() {
                let weak = weak.clone();
                glib::timeout_add_local_once(Duration::from_millis(150), move || {
                    if let Some(ui) = weak.upgrade()
                        && !ui.window.is_active()
                        && !ui.modal.get()
                        && !ui.action_button.is_active()
                        && ui
                            .settings_window
                            .borrow()
                            .as_ref()
                            .is_none_or(|w| !w.is_visible())
                    {
                        ui.hide();
                    }
                });
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.app_monitor.connect_changed(move |_| {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(source) = ui.app_reload.borrow_mut().take() {
                source.remove();
            }
            let weak = weak.clone();
            let source = glib::timeout_add_local_once(Duration::from_millis(500), move || {
                let Some(ui) = weak.upgrade() else { return };
                ui.app_reload.borrow_mut().take();
                // Listing the apps again also re-arms the monitor for the next change.
                *ui.apps.borrow_mut() = apps::installed();
                ui.catalog.borrow_mut().take();
                if ui.active.borrow().is_none() {
                    ui.render_search();
                }
            });
            *ui.app_reload.borrow_mut() = Some(source);
        });
        let runtime = ui.runtime.clone();
        glib::timeout_add_seconds_local(30, move || {
            runtime.reap();
            glib::ControlFlow::Continue
        });
        ui.render_search();
        ui
    }
    fn show(self: &Rc<Self>) {
        let start = Instant::now();
        if let Some(fade) = self.closing.borrow_mut().take() {
            fade.pause();
        }
        self.window.set_opacity(1.);
        self.back();
        self.window.present();
        self.entry.grab_focus();
        if std::env::var_os("LUMEN_PROFILE").is_some() {
            eprintln!(
                "Lumen present requested in {:.2} ms",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
    fn toggle(self: &Rc<Self>) {
        if self.window.is_visible() && !self.is_closing() {
            self.hide();
        } else {
            self.show();
        }
    }
    fn is_closing(&self) -> bool {
        self.closing
            .borrow()
            .as_ref()
            .is_some_and(|fade| fade.state() == adw::AnimationState::Playing)
    }
    fn cancel(&self) {
        if let Some(cancel) = self.file_cancel.borrow_mut().take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.command_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(source) = self.debounce.borrow_mut().take() {
            source.remove();
        }
        if let Some(source) = self.refresh.borrow_mut().take() {
            source.remove();
        }
        if let Some(source) = self.select_debounce.borrow_mut().take() {
            source.remove();
        }
        self.generation.set(self.generation.get().wrapping_add(1));
    }
    /// Fade out briefly, then unmap. The Shell side animates opening on the
    /// compositor; a Wayland client must animate its own close before unmapping.
    /// TimedAnimation skips straight to the end when animations are disabled.
    fn hide(&self) {
        self.cancel();
        self.action_button.popdown();
        if !self.window.is_visible() || self.is_closing() {
            return;
        }
        let fade = adw::TimedAnimation::builder()
            .widget(&self.window)
            .value_from(self.window.opacity())
            .value_to(0.)
            .duration(110)
            .easing(adw::Easing::EaseInQuad)
            .target(&adw::PropertyAnimationTarget::new(&self.window, "opacity"))
            .build();
        let window = self.window.downgrade();
        fade.connect_done(move |_| {
            if let Some(window) = window.upgrade() {
                window.set_visible(false);
                window.set_opacity(1.);
            }
        });
        *self.closing.borrow_mut() = Some(fade.clone());
        fade.play();
    }
    fn back(self: &Rc<Self>) {
        self.cancel();
        self.active.borrow_mut().take();
        *self.view.borrow_mut() = Value::Null;
        self.views.borrow_mut().clear();
        self.pushed.set(None);
        *self.launch.borrow_mut() = json!({});
        self.no_view.set(false);
        self.grid.borrow_mut().take();
        self.selected_id.borrow_mut().clear();
        self.clear_toast();
        self.form.borrow_mut().clear();
        self.stack.set_visible_child_name("results");
        self.side_scroll.set_visible(false);
        self.scope.set_visible(false);
        self.entry.set_placeholder_text(Some(PLACEHOLDER));
        self.suppress_search.set(true);
        self.entry.set_text("");
        self.suppress_search.set(false);
        self.file_results.borrow_mut().clear();
        self.render_search();
    }
    /// Esc inside a command: return to the view it pushed over, else to the root.
    fn go_back(self: &Rc<Self>) {
        let Some((view, query)) = self.views.borrow_mut().pop() else {
            self.back();
            return;
        };
        self.cancel();
        self.pushed.set(None);
        self.selected_id.borrow_mut().clear();
        self.suppress_search.set(true);
        self.entry.set_text(&query);
        self.entry.set_position(-1);
        self.suppress_search.set(false);
        // Force a rebuild: the popped view may share row IDs with the pushed one.
        *self.results.borrow_mut() = Vec::new();
        self.render_view(view);
        self.status.set_label("Esc to go back");
        self.schedule_refresh();
    }
    /// The highlighted list row or grid tile, on whichever page is showing.
    fn selected_index(&self) -> Option<usize> {
        match self.stack.visible_child_name().as_deref() {
            Some("results") => self.list.selected_row().map(|r| r.index() as usize),
            Some("grid") => self
                .grid
                .borrow()
                .as_ref()
                .filter(|g| !g.tiles.is_empty())
                .map(|_| self.grid_selected.get()),
            _ => None,
        }
    }
    fn search_changed(self: &Rc<Self>) {
        self.cancel();
        if self.active.borrow().is_some() {
            let weak = Rc::downgrade(self);
            let source = glib::timeout_add_local_once(Duration::from_millis(100), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.debounce.borrow_mut().take();
                    ui.extension_request(
                        "command.query",
                        json!({"query":ui.entry.text().as_str()}),
                    );
                }
            });
            *self.debounce.borrow_mut() = Some(source);
            return;
        }
        let query = self.entry.text().to_string();
        // Keep file hits that still match while LocalSearch answers the new query,
        // so the list narrows instead of flickering empty on every keystroke.
        let needle = query.trim().to_lowercase();
        self.file_results
            .borrow_mut()
            .retain(|c| !needle.is_empty() && c.title.contains(&needle));
        self.render_search();
        if needle.is_empty() || query.len() > 256 {
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(Duration::from_millis(60), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.debounce.borrow_mut().take();
            let generation = ui.generation.get();
            let cancel = gio::Cancellable::new();
            *ui.file_cancel.borrow_mut() = Some(cancel.clone());
            let started = Instant::now();
            let weak = Rc::downgrade(&ui);
            // Await the worker instead of polling the main loop for its result.
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    files::search(&query, &cancel).map_err(|e| e.to_string())
                })
                .await;
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if ui.generation.get() != generation {
                    return;
                }
                ui.file_cancel.borrow_mut().take();
                match result {
                    Ok(Ok(items)) => {
                        *ui.file_results.borrow_mut() =
                            items.into_iter().map(Candidate::new).collect();
                        ui.render_search();
                        if std::env::var_os("LUMEN_PROFILE").is_some() {
                            eprintln!(
                                "Lumen file results: {:.2} ms",
                                started.elapsed().as_secs_f64() * 1000.
                            );
                        }
                    }
                    _ => ui
                        .status
                        .set_label("File index unavailable · Check Settings"),
                }
            });
        });
        *self.debounce.borrow_mut() = Some(source);
    }
    fn catalog(&self) -> Rc<Vec<Candidate>> {
        if let Some(catalog) = self.catalog.borrow().as_ref() {
            return catalog.clone();
        }
        let state = self.state.borrow();
        let mut candidates: Vec<_> = self
            .apps
            .borrow()
            .iter()
            .cloned()
            .map(Candidate::new)
            .collect();
        for ext in self
            .extensions
            .borrow()
            .iter()
            .filter(|e| state.is_enabled(e))
        {
            for cmd in &ext.manifest.commands {
                candidates.push(Candidate::new(Item {
                    id: format!("ext:{}:{}", ext.manifest.id, cmd.id),
                    title: cmd.title.clone(),
                    subtitle: if cmd.subtitle.is_empty() {
                        ext.manifest.name.clone()
                    } else {
                        cmd.subtitle.clone()
                    },
                    icon: if cmd.icon.is_empty() {
                        "application-x-addon-symbolic".into()
                    } else {
                        cmd.icon.clone()
                    },
                    keywords: cmd.keywords.join(" "),
                    kind: Kind::Command {
                        extension: ext.manifest.id.clone(),
                        command: cmd.id.clone(),
                    },
                }));
            }
        }
        candidates.push(Candidate::new(Item {
            id: "settings".into(),
            title: "Lumen Settings".into(),
            subtitle: "Extensions, keyboard shortcut, and file search".into(),
            icon: "emblem-system-symbolic".into(),
            keywords: "preferences extensions install index shortcut".into(),
            kind: Kind::Settings,
        }));
        let catalog = Rc::new(candidates);
        *self.catalog.borrow_mut() = Some(catalog.clone());
        catalog
    }
    fn render_search(self: &Rc<Self>) {
        if self.active.borrow().is_some() {
            return;
        }
        let query = self.entry.text();
        let query = query.trim();
        let needle = query.to_lowercase();
        let catalog = self.catalog();
        let files = self.file_results.borrow();
        let state = self.state.borrow();
        // Score by reference; only the few shown items are cloned.
        let mut scored: Vec<_> = catalog
            .iter()
            .chain(files.iter())
            .filter_map(|c| {
                model::score(&needle, &c.title, &c.keywords).map(|score| {
                    (
                        score,
                        state.favorites.contains(&c.item.id),
                        state.usage.get(&c.item.id).copied().unwrap_or(0),
                        c,
                    )
                })
            })
            .collect();
        let limit = if query.is_empty() {
            SUGGESTION_LIMIT
        } else {
            RESULT_LIMIT
        };
        // Apps-first also wins the result limit, so a strong file match never
        // pushes a weaker app match out of the list.
        let apps_first = !query.is_empty() && state.result_order == ResultOrder::AppsFirst;
        fn rank<'a>(x: &(u32, bool, u32, &'a Candidate)) -> model::Rank<'a> {
            model::Rank {
                score: x.0,
                favorite: x.1,
                usage: x.2,
                app: matches!(x.3.item.kind, Kind::App(_)),
                title: &x.3.title,
            }
        }
        let order = |a: &(u32, bool, u32, &Candidate), b: &(u32, bool, u32, &Candidate)| {
            model::compare(&rank(a), &rank(b), apps_first)
        };
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, order);
            scored.truncate(limit);
        }
        scored.sort_unstable_by(order);
        // Keep provider sections together, ordered by each section's best match.
        let mut groups: Vec<(&'static str, Vec<Item>)> = Vec::new();
        for (_, favorite, _, c) in scored {
            let name = if query.is_empty() {
                if favorite { "Favorites" } else { "Suggestions" }
            } else {
                section(&c.item.kind)
            };
            match groups.iter_mut().find(|(key, _)| *key == name) {
                Some((_, items)) => items.push(c.item.clone()),
                None => groups.push((name, vec![c.item.clone()])),
            }
        }
        drop((state, files));
        // With several providers, cap each one so a long app list cannot push
        // every file (or command) below the fold.
        if groups.len() > 1 {
            for (_, items) in &mut groups {
                items.truncate(SECTION_LIMIT);
            }
        }
        let (mut sections, mut items): (Vec<_>, Vec<_>) = groups
            .into_iter()
            .flat_map(|(name, items)| items.into_iter().map(move |item| (name, item)))
            .unzip();
        let mut calc_error = None;
        match calculator::calculate(query) {
            Ok(Some(value)) => {
                sections.insert(0, section(&Kind::Calculation(String::new())));
                items.insert(
                    0,
                    Item {
                        id: "calculator".into(),
                        title: value.clone(),
                        subtitle: format!("= {query}"),
                        icon: "accessories-calculator-symbolic".into(),
                        keywords: String::new(),
                        kind: Kind::Calculation(value),
                    },
                )
            }
            Err(e)
                if query.starts_with('=')
                    || query
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || "(-+".contains(c)) =>
            {
                calc_error = Some(e.to_string())
            }
            _ => {}
        }
        let count = items.len();
        self.status.set_label(&match calc_error {
            Some(e) => e,
            None if query.is_empty() => "Type to search apps, files and commands".into(),
            None if count == 0 => String::new(),
            None if count == 1 => "1 result".into(),
            None => format!("{count} results"),
        });
        let count = items.len();
        self.set_items(
            items,
            sections.into_iter().map(String::from).collect(),
            vec![Value::Null; count],
        );
    }
    fn set_items(self: &Rc<Self>, items: Vec<Item>, sections: Vec<String>, decor: Vec<Value>) {
        let favorites: Vec<bool> = {
            let state = self.state.borrow();
            items
                .iter()
                .map(|i| state.favorites.contains(&i.id))
                .collect()
        };
        // File results often arrive identical to what is already shown; rebuilding
        // the rows would reload icons and reset hover for nothing.
        if *self.results.borrow() == items
            && {
                let rendered = self.rendered.borrow();
                rendered.0 == sections && rendered.1 == favorites && rendered.2 == decor
            }
            && (!items.is_empty() || self.list.row_at_index(0).is_some())
        {
            self.update_hints();
            return;
        }
        // A refreshed list with the same rows only swaps row contents, so the
        // selection, hover and scroll position stay where the user left them.
        let same_rows = !items.is_empty()
            && self.rendered.borrow().0 == sections
            && self.results.borrow().len() == items.len()
            && self.results.borrow().iter().zip(&items).all(|(a, b)| a.id == b.id);
        if same_rows {
            for (index, item) in items.iter().enumerate() {
                if let Some(row) = self.list.row_at_index(index as i32) {
                    row.set_child(Some(&result_row(item, favorites[index], &decor[index])));
                }
            }
            *self.results.borrow_mut() = items;
            *self.rendered.borrow_mut() = (sections, favorites, decor);
            self.update_hints();
            self.update_side();
            return;
        }
        let selected = self.list.selected_row().and_then(|r| {
            self.results
                .borrow()
                .get(r.index() as usize)
                .map(|i| i.id.clone())
        });
        self.list.remove_all();
        for (index, item) in items.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("result-row");
            if let Some(name) = sections.get(index) {
                row.set_widget_name(&format!("section:{name}"));
            }
            row.set_child(Some(&result_row(item, favorites[index], &decor[index])));
            self.list.append(&row);
        }
        let empty = items.is_empty();
        *self.results.borrow_mut() = items;
        *self.rendered.borrow_mut() = (sections, favorites, decor);
        if empty {
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            row.set_activatable(false);
            row.set_child(Some(&self.empty_state()));
            self.list.append(&row);
        } else {
            let index = selected
                .and_then(|id| self.results.borrow().iter().position(|i| i.id == id))
                .unwrap_or(0);
            if let Some(row) = self.list.row_at_index(index as i32) {
                self.list.select_row(Some(&row));
                if index == 0 {
                    self.scroll.vadjustment().set_value(0.);
                } else {
                    // Rows have no allocation until the next layout pass.
                    let weak = Rc::downgrade(self);
                    glib::idle_add_local_once(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.reveal(&row);
                        }
                    });
                }
            }
        }
        self.update_hints();
        self.update_side();
    }
    /// Shows the selected extension row's `detail` in the side pane.
    fn update_side(&self) {
        if !self.side_scroll.is_visible() {
            return;
        }
        let index = self.list.selected_row().map(|r| r.index() as usize);
        let rendered = self.rendered.borrow();
        match index.and_then(|i| rendered.2.get(i)) {
            Some(item) if !item.is_null() => rich::fill_detail(&self.side, item),
            _ => rich::fill_detail(&self.side, &Value::Null),
        }
    }
    fn empty_state(&self) -> gtk::Box {
        let query = self.entry.text();
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 6);
        empty.add_css_class("empty-state");
        let custom = self.view.borrow()["empty"].clone();
        if self.active.borrow().is_some() && custom["title"].is_string() {
            let icon = gtk::Image::from_icon_name(
                custom["icon"].as_str().unwrap_or("system-search-symbolic"),
            );
            icon.set_pixel_size(48);
            icon.add_css_class("empty-icon");
            empty.append(&icon);
            let title = gtk::Label::builder()
                .label(custom["title"].as_str().unwrap_or(""))
                .wrap(true)
                .justify(gtk::Justification::Center)
                .build();
            title.add_css_class("empty-title");
            empty.append(&title);
            if let Some(text) = custom["description"].as_str() {
                let hint = gtk::Label::builder()
                    .label(text)
                    .wrap(true)
                    .justify(gtk::Justification::Center)
                    .build();
                hint.add_css_class("empty-hint");
                empty.append(&hint);
            }
            return empty;
        }
        let icon = gtk::Image::from_icon_name("system-search-symbolic");
        icon.set_pixel_size(48);
        icon.add_css_class("empty-icon");
        empty.append(&icon);
        let title = gtk::Label::builder()
            .label(if query.trim().is_empty() {
                "Nothing here yet".to_owned()
            } else {
                format!("No results for “{}”", query.trim())
            })
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(48)
            .build();
        title.add_css_class("empty-title");
        empty.append(&title);
        let hint = gtk::Label::new(Some(if self.active.borrow().is_some() {
            "Try a different search, or press Esc to go back"
        } else {
            "Check the spelling, or try an app, file or calculation"
        }));
        hint.add_css_class("empty-hint");
        empty.append(&hint);
        empty
    }
    /// Updates the footer to describe what Enter will do for the selection.
    fn update_hints(&self) {
        let verb = self.selected_index().and_then(|index| {
            self.results.borrow().get(index).map(|item| {
                match (&item.kind, self.active.borrow().is_some()) {
                    // An extension names its Enter action as the row's first action.
                    (_, true) => self.rendered.borrow().2.get(index)
                        .and_then(|i| i["actions"][0]["title"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| "Select".into()),
                    (Kind::Calculation(_), _) => "Copy".into(),
                    (Kind::Command { .. }, _) => {
                        if self.arguments.is_visible() { "Run with Arguments".into() } else { "Run".into() }
                    }
                    _ => "Open".into(),
                }
            })
        });
        let results = matches!(self.stack.visible_child_name().as_deref(), Some("results" | "grid"));
        self.primary_hint.set_visible(verb.is_some() && results);
        if let Some(verb) = verb {
            self.primary_label.set_label(&verb);
        }
    }
    fn move_selection(&self, delta: i32) {
        let count = self.results.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let index = match self.list.selected_row() {
            None => 0,
            // Single steps wrap around; page jumps stop at the ends.
            Some(row) if delta.abs() == 1 => (row.index() + delta).rem_euclid(count),
            Some(row) => (row.index() + delta).clamp(0, count - 1),
        };
        if let Some(row) = self.list.row_at_index(index) {
            self.list.select_row(Some(&row));
            // Scroll without moving focus: focusing the row and returning to the
            // entry would select the whole query, so the next key replaced it.
            self.reveal(&row);
        }
    }
    fn reveal(&self, row: &gtk::ListBoxRow) {
        let adjustment = self.scroll.vadjustment();
        let top_widget = row.header().unwrap_or_else(|| row.clone().upcast());
        let (Some(top), Some(bottom)) = (
            top_widget.compute_bounds(&self.list),
            row.compute_bounds(&self.list),
        ) else {
            return;
        };
        let start = f64::from(top.y());
        let end = f64::from(bottom.y() + bottom.height());
        let page = adjustment.page_size();
        if start < adjustment.value() {
            adjustment.set_value(start);
        } else if end > adjustment.value() + page {
            adjustment.set_value(end - page + 8.);
        }
    }
    fn activate(self: &Rc<Self>, index: usize) {
        let Some(item) = self.results.borrow().get(index).cloned() else {
            return;
        };
        if self.active.borrow().is_some() {
            self.extension_request("command.action", json!({"item":item.id,"action":"primary"}));
            return;
        }
        let result: anyhow::Result<()> = (|| {
            match &item.kind {
                Kind::App(id) => {
                    let app = gio::AppInfo::all()
                        .into_iter()
                        .find(|app| app.id().as_deref() == Some(id.as_str()))
                        .ok_or_else(|| anyhow::anyhow!("Application is no longer installed"))?;
                    let ctx = gtk::prelude::WidgetExt::display(&self.window).app_launch_context();
                    app.launch(&[], Some(&ctx))?;
                    self.hide();
                }
                Kind::File(uri) => {
                    self.open_uri(uri)?;
                    self.hide();
                }
                Kind::Calculation(value) => {
                    self.window.clipboard().set_text(value);
                    self.status
                        .set_label(&format!("Copied {value} to the clipboard"));
                }
                Kind::Settings => self.open_settings(),
                Kind::Command { extension, command } => {
                    let Some(arguments) = self.argument_values(&item.id) else {
                        return Ok(());
                    };
                    self.open_command(extension, command, json!({ "arguments": arguments }))?
                }
            }
            self.state.borrow_mut().used(&item.id);
            self.persist();
            Ok(())
        })();
        if let Err(e) = result {
            self.status.set_label(&format!("{e:#}"));
        }
    }
    /// Enters an extension command, as if its result had been chosen.
    /// `launch` carries its `arguments` and `launch_context`.
    fn open_command(self: &Rc<Self>, extension: &str, command: &str, launch: Value) -> anyhow::Result<()> {
        let ext = self
            .extensions
            .borrow()
            .iter()
            .find(|e| e.manifest.id == extension && self.state.borrow().is_enabled(e))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Extension is not installed or turned off"))?;
        let manifest = ext
            .manifest
            .commands
            .iter()
            .find(|c| c.id == command)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Extension has no command {command}"))?;
        if let Some(missing) = self.state.borrow().missing_preference(&ext, command) {
            let id = ext.manifest.id.clone();
            self.open_settings_at(&id);
            anyhow::bail!("Set “{missing}” in the extension’s settings first");
        }
        let title = manifest.title;
        self.cancel();
        self.views.borrow_mut().clear();
        self.pushed.set(None);
        self.suppress_search.set(true);
        self.entry.set_text("");
        self.suppress_search.set(false);
        self.arguments.set_visible(false);
        *self.active.borrow_mut() = Some((ext, command.to_owned()));
        *self.launch.borrow_mut() = launch;
        self.no_view.set(manifest.mode == Mode::NoView);
        if manifest.mode == Mode::NoView {
            // Nothing to show: run it, then close (see `finish`).
            self.extension_request("command.run", json!({"query":""}));
            return Ok(());
        }
        self.entry
            .set_placeholder_text(Some(&format!("Search {title}…")));
        self.scope.set_child(Some(&{
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
            content.append(
                &gtk::Label::builder()
                    .label(&title)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(22)
                    .build(),
            );
            content
        }));
        self.scope.set_visible(true);
        self.extension_request("command.run", json!({"query":""}));
        Ok(())
    }
    /// Shows inline fields for the selected root command's arguments.
    fn update_arguments(self: &Rc<Self>) {
        let command = (self.active.borrow().is_none())
            .then(|| self.list.selected_row())
            .flatten()
            .and_then(|r| self.results.borrow().get(r.index() as usize).cloned())
            .and_then(|item| match &item.kind {
                Kind::Command { extension, command } => self
                    .extensions
                    .borrow()
                    .iter()
                    .find(|e| &e.manifest.id == extension)
                    .and_then(|e| e.manifest.commands.iter().find(|c| &c.id == command).cloned())
                    .filter(|c| !c.arguments.is_empty())
                    .map(|c| (item.id.clone(), c)),
                _ => None,
            });
        let Some((owner, command)) = command else {
            self.arguments.set_visible(false);
            self.argument_owner.borrow_mut().clear();
            return;
        };
        if *self.argument_owner.borrow() == owner {
            self.arguments.set_visible(true);
            return;
        }
        while let Some(child) = self.arguments.first_child() {
            self.arguments.remove(&child);
        }
        let mut fields = Vec::new();
        for argument in &command.arguments {
            let placeholder = if argument.required {
                argument.placeholder.clone()
            } else {
                format!("{} (optional)", argument.placeholder)
            };
            let (widget, values): (gtk::Widget, Vec<String>) = if argument.kind == "dropdown" {
                let titles: Vec<&str> = argument.data.iter().map(|c| c.title.as_str()).collect();
                let dropdown = gtk::DropDown::from_strings(&titles);
                dropdown.set_tooltip_text(Some(&argument.placeholder));
                (dropdown.upcast(), argument.data.iter().map(|c| c.value.clone()).collect())
            } else {
                let entry = gtk::Entry::builder()
                    .placeholder_text(&placeholder)
                    .width_chars(placeholder.chars().count().clamp(6, 16) as i32)
                    .visibility(argument.kind != "password")
                    .build();
                let weak = Rc::downgrade(self);
                entry.connect_activate(move |_| {
                    if let Some(ui) = weak.upgrade()
                        && let Some(row) = ui.list.selected_row()
                    {
                        ui.activate(row.index() as usize);
                    }
                });
                (entry.upcast(), Vec::new())
            };
            widget.add_css_class("argument");
            self.arguments.append(&widget);
            fields.push((argument.name.clone(), widget, argument.required, values));
        }
        *self.argument_fields.borrow_mut() = fields;
        *self.argument_owner.borrow_mut() = owner;
        self.arguments.set_visible(true);
        self.update_hints();
    }
    /// Moves focus through the search field and the argument fields.
    fn cycle_arguments(&self, step: i32) {
        let fields = self.argument_fields.borrow();
        let focus: Vec<gtk::Widget> = std::iter::once(self.entry.clone().upcast())
            .chain(fields.iter().map(|f| f.1.clone()))
            .collect();
        let at = focus
            .iter()
            .position(|w| w.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN))
            .unwrap_or(0) as i32;
        let next = (at + step).rem_euclid(focus.len() as i32) as usize;
        focus[next].grab_focus();
    }
    /// The typed arguments for `owner`, or None after focusing a missing one.
    fn argument_values(&self, owner: &str) -> Option<serde_json::Map<String, Value>> {
        let mut values = serde_json::Map::new();
        if *self.argument_owner.borrow() != owner || !self.arguments.is_visible() {
            return Some(values);
        }
        for (name, widget, required, choices) in self.argument_fields.borrow().iter() {
            if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
                let text = entry.text();
                if *required && text.trim().is_empty() {
                    entry.grab_focus();
                    self.status.set_label("Fill in the required arguments, then press Enter");
                    return None;
                }
                values.insert(name.clone(), json!(text.as_str()));
            } else if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
                let value = choices.get(dropdown.selected() as usize).cloned().unwrap_or_default();
                values.insert(name.clone(), json!(value));
            }
        }
        Some(values)
    }
    fn move_grid(self: &Rc<Self>, dx: i32, dy: i32) {
        let next = match self.grid.borrow().as_ref() {
            Some(g) if !g.tiles.is_empty() => {
                grid::neighbour(&g.sections, g.columns, self.grid_selected.get(), dx, dy)
            }
            _ => return,
        };
        self.select_tile(next);
    }
    fn select_tile(self: &Rc<Self>, index: usize) {
        {
            let grid = self.grid.borrow();
            let Some(grid) = grid.as_ref() else { return };
            if let Some(old) = grid.tiles.get(self.grid_selected.get()) {
                old.remove_css_class("selected");
            }
            let Some(tile) = grid.tiles.get(index) else { return };
            tile.add_css_class("selected");
            self.grid_selected.set(index);
            // Scroll the tile into view without moving focus out of the search field.
            let adjustment = self.grid_scroll.vadjustment();
            if let Some(bounds) = tile.compute_bounds(&grid.root) {
                let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
                if top < adjustment.value() {
                    adjustment.set_value((top - 28.).max(0.));
                } else if bottom > adjustment.value() + adjustment.page_size() {
                    adjustment.set_value(bottom - adjustment.page_size() + 12.);
                }
            }
        }
        self.update_hints();
        self.selection_changed();
    }
    /// Views with `selection: true` hear which item is highlighted, after a pause.
    fn selection_changed(self: &Rc<Self>) {
        if self.active.borrow().is_none() || self.view.borrow()["selection"] != true {
            return;
        }
        let Some(id) = self
            .selected_index()
            .and_then(|i| self.results.borrow().get(i).map(|item| item.id.clone()))
        else {
            return;
        };
        if *self.selected_id.borrow() == id {
            return;
        }
        *self.selected_id.borrow_mut() = id.clone();
        if let Some(source) = self.select_debounce.borrow_mut().take() {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(Duration::from_millis(500), move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.select_debounce.borrow_mut().take();
            if *ui.selected_id.borrow() == id && !ui.action_button.is_active() {
                ui.extension_request_with("command.select", json!({ "item": id }), true);
            }
        });
        *self.select_debounce.borrow_mut() = Some(source);
    }
    fn open_uri(&self, uri: &str) -> anyhow::Result<()> {
        gio::AppInfo::launch_default_for_uri(
            uri,
            Some(&gtk::prelude::WidgetExt::display(&self.window).app_launch_context()),
        )?;
        Ok(())
    }
    fn persist(&self) {
        if let Err(e) = self.state.borrow().save() {
            self.status
                .set_label(&format!("Cannot save preferences: {e}"));
        }
    }
    pub(super) fn refresh_extensions(self: &Rc<Self>) {
        *self.extensions.borrow_mut() = package::discover(&self.state.borrow());
        self.catalog.borrow_mut().take();
        if self.active.borrow().is_none() {
            self.render_search();
        }
    }
    fn build_actions(self: &Rc<Self>) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.add_css_class("menu");
        popover.add_css_class("actions-menu");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let add = |label: &str, icon: &str, run: MenuAction| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.append(&gtk::Image::from_icon_name(icon));
            row.append(&gtk::Label::builder().label(label).xalign(0.).build());
            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("flat");
            button.add_css_class("action-item");
            let weak = Rc::downgrade(self);
            let pop = popover.downgrade();
            button.connect_clicked(move |_| {
                if let Some(pop) = pop.upgrade() {
                    pop.popdown();
                }
                if let Some(ui) = weak.upgrade() {
                    run(&ui);
                }
            });
            content.append(&button);
        };
        let selected = self
            .selected_index()
            .and_then(|i| self.results.borrow().get(i).cloned());
        if let Some(item) = selected {
            if self.active.borrow().is_some() {
                let actions = self.view.borrow()["items"]
                    .as_array()
                    .and_then(|items| items.iter().find(|i| i["id"] == item.id))
                    .and_then(|i| i["actions"].as_array())
                    .cloned()
                    .unwrap_or_default();
                for action in actions.iter().take(12) {
                    let label = action["title"].as_str().unwrap_or("Action");
                    let id = action["id"].as_str().unwrap_or("primary").to_owned();
                    let item_id = item.id.clone();
                    add(
                        label,
                        "media-playback-start-symbolic",
                        Box::new(move |ui| {
                            ui.extension_request(
                                "command.action",
                                json!({"item":item_id,"action":id}),
                            )
                        }),
                    );
                }
            } else {
                let favorite = self.state.borrow().favorites.contains(&item.id);
                let id = item.id.clone();
                add(
                    if favorite {
                        "Remove from Favorites"
                    } else {
                        "Add to Favorites"
                    },
                    if favorite {
                        "starred-symbolic"
                    } else {
                        "non-starred-symbolic"
                    },
                    Box::new(move |ui| {
                        {
                            let mut state = ui.state.borrow_mut();
                            if favorite {
                                state.favorites.remove(&id);
                            } else {
                                state.favorites.insert(id.clone());
                            }
                        }
                        ui.persist();
                        ui.render_search();
                    }),
                );
                if let Kind::File(uri) = item.kind {
                    let reveal_uri = uri.clone();
                    add(
                        "Show in Files",
                        "folder-open-symbolic",
                        Box::new(move |ui| {
                            reveal_in_files(reveal_uri.clone());
                            ui.hide();
                        }),
                    );
                    add(
                        "Copy Path",
                        "edit-copy-symbolic",
                        Box::new(move |ui| {
                            if let Some(path) = gio::File::for_uri(&uri).path() {
                                ui.window.clipboard().set_text(&path.display().to_string());
                                ui.status.set_label("Copied path to the clipboard");
                            }
                        }),
                    );
                }
            }
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        add(
            "Settings",
            "emblem-system-symbolic",
            Box::new(|ui| ui.open_settings()),
        );
        popover.set_child(Some(&content));
        popover
    }
    fn extension_request(self: &Rc<Self>, method: &str, params: Value) {
        self.extension_request_with(method, params, false);
    }
    /// `quiet` requests are timed refreshes and selection events: no
    /// "Running command…" status.
    fn extension_request_with(self: &Rc<Self>, method: &str, mut params: Value, quiet: bool) {
        let Some((ext, command)) = self.active.borrow().clone() else {
            return;
        };
        self.cancel();
        let generation = self.generation.get();
        params["command"] = json!(command);
        params["query"] = json!(self.entry.text().as_str());
        params["preferences"] = Value::Object(self.state.borrow().preference_values(&ext, &command));
        let launch = self.launch.borrow();
        params["arguments"] = launch.get("arguments").cloned().unwrap_or_else(|| json!({}));
        params["launch_context"] = launch.get("launch_context").cloned().unwrap_or(Value::Null);
        drop(launch);
        let context = &self.view.borrow()["context"];
        if !context.is_null() {
            params["context"] = context.clone();
        }
        // Only an action or a form submission may open a new view over this one.
        let may_push = matches!(method, "command.action" | "command.submit");
        let cancel = Arc::new(AtomicBool::new(false));
        *self.command_cancel.borrow_mut() = Some(cancel.clone());
        let runtime = self.runtime.clone();
        let method = method.to_owned();
        // The worker wakes the main loop only when it has an event, instead of
        // the UI polling a channel every 12 ms until the command finishes.
        let (tx, mut rx) = futures_channel::mpsc::unbounded();
        if !quiet {
            self.clear_toast();
            self.status.set_label(if self.no_view.get() { "Running…" } else { "Running command…" });
        }
        self.failed.set(false);
        self.pending_hud.borrow_mut().take();
        self.pending_close.set(false);
        std::thread::spawn(move || {
            let events = tx.clone();
            let result = runtime.request(&ext, &method, params, cancel, |event| {
                let _ = events.unbounded_send(Ok((false, event)));
            });
            let _ = tx.unbounded_send(
                result
                    .map(|view| (true, json!({"type":"view","view":view})))
                    .map_err(|e| e.to_string()),
            );
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            use futures_util::StreamExt;
            while let Some(event) = rx.next().await {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if ui.generation.get() != generation {
                    return;
                }
                match event {
                    Ok((done, event)) => {
                        if event["type"] == "view" && !event["view"].is_null() {
                            let view = event["view"].clone();
                            if may_push && view["push"] == true && ui.pushed.get() != Some(generation) {
                                ui.push_current();
                                ui.pushed.set(Some(generation));
                            }
                            ui.render_view(view);
                        } else if event["type"] == "effect" {
                            ui.effect(&event);
                        } else if event["type"] == "ask" {
                            ui.answer(&event);
                        }
                        if done {
                            ui.finish(quiet);
                            return;
                        }
                    }
                    Err(e) => {
                        if ui.no_view.get() {
                            // Nothing was shown: report it from the root search.
                            ui.back();
                        }
                        ui.spinner.set_visible(false);
                        ui.status.set_label(&format!("Extension: {e}"));
                        return;
                    }
                }
            }
        });
    }
    /// Saves the visible view and its search text, before a pushed view replaces it.
    fn push_current(&self) {
        let view = self.view.borrow().clone();
        if view.is_null() {
            return;
        }
        self.views.borrow_mut().push((view, self.entry.text().to_string()));
        self.suppress_search.set(true);
        self.entry.set_text("");
        self.suppress_search.set(false);
        self.selected_id.borrow_mut().clear();
    }
    /// A request ended: show the HUD or close if it asked to, and close after
    /// a no-view command unless it reported a failure.
    fn finish(self: &Rc<Self>, quiet: bool) {
        self.spinner.set_visible(false);
        let hud = self.pending_hud.borrow_mut().take();
        let close = self.pending_close.get() || hud.is_some() || (self.no_view.get() && !self.failed.get());
        if close {
            self.hide();
            if let Some(text) = hud {
                show_hud(&self.app, &text);
            }
            return;
        }
        if self.no_view.get() {
            // A failure toast is showing; leave it on the root search.
            let status = self.status.label();
            self.back();
            self.status.set_label(&status);
            self.status.add_css_class("toast-failure");
            return;
        }
        if !quiet && !self.toasted.get() {
            self.status.set_label("Esc to go back");
        }
        self.schedule_refresh();
    }
    fn clear_toast(&self) {
        self.toasted.set(false);
        self.spinner.set_visible(false);
        self.spinner.stop();
        for class in ["toast-success", "toast-failure", "toast-animated"] {
            self.status.remove_css_class(class);
        }
    }
    /// Answers a host call that needs the desktop: the clipboard or a file chooser.
    fn answer(self: &Rc<Self>, event: &Value) {
        let Some(id) = event["id"].as_u64() else { return };
        match event["method"].as_str() {
            Some("host.readClipboard") => {
                let clipboard = self.window.clipboard();
                let formats = clipboard.formats();
                if formats.contain_mime_type("x-special/gnome-copied-files")
                    || formats.contain_mime_type("text/uri-list")
                {
                    clipboard.read_async(
                        &["x-special/gnome-copied-files", "text/uri-list"],
                        glib::Priority::DEFAULT,
                        None::<&gio::Cancellable>,
                        move |result| {
                            let Ok((stream, _)) = result else {
                                runtime::answer(id, Ok(json!({})));
                                return;
                            };
                            read_stream(stream, move |bytes| {
                                let files = clipboard_files(&String::from_utf8_lossy(&bytes));
                                runtime::answer(id, Ok(json!({ "files": files })));
                            });
                        },
                    );
                } else if formats.contains_type(gdk::Texture::static_type()) {
                    clipboard.read_texture_async(None::<&gio::Cancellable>, move |result| {
                        let saved = result.ok().flatten().and_then(|texture| {
                            let dir = glib::user_runtime_dir().join("gnome-launcher");
                            std::fs::create_dir_all(&dir).ok()?;
                            let path = dir.join(format!("clipboard-{}.png", glib::monotonic_time()));
                            texture.save_to_png(&path).ok()?;
                            Some(path)
                        });
                        runtime::answer(
                            id,
                            Ok(saved.map_or(json!({}), |p| json!({ "image": p }))),
                        );
                    });
                } else {
                    clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
                        let text = result.ok().flatten().map(|t| t.to_string());
                        runtime::answer(id, Ok(text.map_or(json!({}), |t| json!({ "text": t }))));
                    });
                }
            }
            Some("host.chooseFiles") => {
                let params = &event["params"];
                let dialog = gtk::FileDialog::builder()
                    .title(params["title"].as_str().unwrap_or("Choose Files"))
                    .modal(true)
                    .build();
                if let Some(types) = params["mimeTypes"].as_array().filter(|t| !t.is_empty()) {
                    let filter = gtk::FileFilter::new();
                    filter.set_name(Some(params["filterName"].as_str().unwrap_or("Supported files")));
                    for mime in types.iter().filter_map(Value::as_str) {
                        filter.add_mime_type(mime);
                    }
                    let filters = gio::ListStore::new::<gtk::FileFilter>();
                    filters.append(&filter);
                    dialog.set_filters(Some(&filters));
                    dialog.set_default_filter(Some(&filter));
                }
                self.modal.set(true);
                let weak = Rc::downgrade(self);
                let done = move |paths: Vec<String>| {
                    if let Some(ui) = weak.upgrade() {
                        ui.modal.set(false);
                        ui.window.present();
                    }
                    runtime::answer(id, Ok(json!({ "files": paths })));
                };
                if params["multiple"] == false {
                    dialog.open(Some(&self.window), None::<&gio::Cancellable>, move |result| {
                        done(result.ok().and_then(|f| f.path()).map(|p| p.display().to_string()).into_iter().collect())
                    });
                } else {
                    dialog.open_multiple(Some(&self.window), None::<&gio::Cancellable>, move |result| {
                        let paths = result
                            .ok()
                            .map(|files| {
                                (0..files.n_items())
                                    .filter_map(|i| files.item(i)?.downcast::<gio::File>().ok()?.path())
                                    .map(|p| p.display().to_string())
                                    .collect()
                            })
                            .unwrap_or_default();
                        done(paths)
                    });
                }
            }
            _ => runtime::answer(id, Err("Unsupported host request".into())),
        }
    }
    /// Lists may ask to be queried again after `refresh` ms, for live clocks.
    /// The refresh waits while a menu is open or another view is showing.
    fn schedule_refresh(self: &Rc<Self>) {
        let Some(delay) = self.view.borrow()["refresh"].as_u64() else {
            return;
        };
        if !matches!(self.view.borrow()["type"].as_str(), Some("list" | "grid")) {
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(
            Duration::from_millis(delay.clamp(500, 60_000)),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                ui.refresh.borrow_mut().take();
                let idle = ui.active.borrow().is_some()
                    && ui.window.is_visible()
                    && matches!(ui.stack.visible_child_name().as_deref(), Some("results" | "grid"));
                if !idle {
                    return;
                }
                if ui.action_button.is_active() {
                    // Try again once the actions menu closes.
                    ui.schedule_refresh();
                    return;
                }
                let query = ui.entry.text().to_string();
                ui.extension_request_with("command.query", json!({ "query": query }), true);
            },
        );
        if let Some(old) = self.refresh.borrow_mut().replace(source) {
            old.remove();
        }
    }
    fn effect(self: &Rc<Self>, event: &Value) {
        let text = event["text"].as_str().unwrap_or("");
        let params = &event["params"];
        match event["method"].as_str() {
            Some("host.copy") => self.window.clipboard().set_text(text),
            Some("host.copyFile") => {
                if let Some(path) = params["path"].as_str() {
                    copy_file(&self.window.clipboard(), path);
                }
            }
            Some("host.open") => {
                if let Err(e) = self.open_uri(text) {
                    self.status.set_label(&e.to_string());
                }
            }
            Some("host.notify") => {
                let notification = gio::Notification::new("Lumen");
                notification.set_body(Some(text));
                self.app.send_notification(None, &notification);
            }
            Some("host.toast") => {
                self.clear_toast();
                self.toasted.set(true);
                let style = params["style"].as_str().unwrap_or("success");
                self.status.add_css_class(&format!("toast-{style}"));
                let title = params["title"].as_str().unwrap_or("");
                self.status.set_label(&match params["message"].as_str().filter(|m| !m.is_empty()) {
                    Some(message) => format!("{title} · {message}"),
                    None => title.to_owned(),
                });
                self.status.set_tooltip_text(params["message"].as_str());
                if style == "animated" {
                    self.spinner.set_visible(true);
                    self.spinner.start();
                }
                self.failed.set(style == "failure");
            }
            Some("host.hud") => *self.pending_hud.borrow_mut() = Some(text.to_owned()),
            Some("host.closeWindow") => self.pending_close.set(true),
            Some("host.openPreferences") => {
                if let Some(id) = self.active.borrow().as_ref().map(|(e, _)| e.manifest.id.clone()) {
                    self.open_settings_at(&id);
                }
            }
            _ => {}
        }
    }
    fn render_view(self: &Rc<Self>, view: Value) {
        *self.view.borrow_mut() = view.clone();
        self.form.borrow_mut().clear();
        if let Some(placeholder) = view["placeholder"].as_str() {
            self.entry.set_placeholder_text(Some(placeholder));
        }
        let items: Vec<Item> = view["items"]
            .as_array()
            .into_iter()
            .flatten()
            .take(100)
            .map(|i| Item {
                id: i["id"].as_str().unwrap_or("").into(),
                title: i["title"].as_str().unwrap_or("").into(),
                subtitle: i["subtitle"].as_str().unwrap_or("").into(),
                icon: i["icon"]
                    .as_str()
                    .unwrap_or("application-x-addon-symbolic")
                    .into(),
                keywords: String::new(),
                kind: Kind::Command {
                    extension: String::new(),
                    command: String::new(),
                },
            })
            .collect();
        let raw: Vec<Value> = view["items"].as_array().into_iter().flatten().take(100).cloned().collect();
        match view["type"].as_str() {
            Some("list") => {
                self.grid.borrow_mut().take();
                let sections = raw
                    .iter()
                    .map(|i| i["section"].as_str().unwrap_or("").to_owned())
                    .collect();
                self.side_scroll.set_visible(view["detail"].as_bool() == Some(true));
                self.stack.set_visible_child_name("results");
                self.set_items(items, sections, raw);
                self.selection_changed();
            }
            Some("grid") => self.render_grid(&view, items, raw),
            Some("detail") | Some("form") => {
                self.grid.borrow_mut().take();
                self.side_scroll.set_visible(false);
                while let Some(child) = self.detail.first_child() {
                    self.detail.remove(&child);
                }
                let title = gtk::Label::builder()
                    .label(view["title"].as_str().unwrap_or("Command"))
                    .xalign(0.)
                    .wrap(true)
                    .build();
                title.add_css_class("title-2");
                title.add_css_class("detail-title");
                self.detail.append(&title);
                if view["type"] == "detail" {
                    if let Some(path) = view["image"].as_str() {
                        let picture = rich::picture(path, 640, 240);
                        picture.set_halign(gtk::Align::Start);
                        self.detail.append(&picture);
                    }
                    let label = gtk::Label::builder()
                        .label(view["text"].as_str().unwrap_or(""))
                        .wrap(true)
                        .selectable(true)
                        .xalign(0.)
                        .yalign(0.)
                        .build();
                    label.add_css_class("detail-body");
                    self.detail.append(&label);
                    if let Some(grid) = rich::metadata_grid(&view["metadata"]) {
                        self.detail.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                        self.detail.append(&grid);
                    }
                } else {
                    self.render_form(&view);
                }
                self.stack.set_visible_child_name("detail");
                self.update_hints();
            }
            _ => {}
        }
    }
    fn render_grid(self: &Rc<Self>, view: &Value, items: Vec<Item>, raw: Vec<Value>) {
        self.side_scroll.set_visible(false);
        let keep = self
            .selected_index()
            .filter(|_| self.stack.visible_child_name().as_deref() == Some("grid"))
            .and_then(|i| self.results.borrow().get(i).map(|item| item.id.clone()));
        let weak = Rc::downgrade(self);
        let built = grid::build(view, move |index| {
            if let Some(ui) = weak.upgrade() {
                ui.select_tile(index);
                ui.activate(index);
            }
        });
        let empty = items.is_empty();
        let index = keep
            .and_then(|id| items.iter().position(|i| i.id == id))
            .unwrap_or(0);
        *self.results.borrow_mut() = items;
        *self.rendered.borrow_mut() = (Vec::new(), Vec::new(), raw);
        let scroll = self.grid_scroll.vadjustment().value();
        if empty {
            self.grid_scroll.set_child(Some(&self.empty_state()));
        } else {
            self.grid_scroll.set_child(Some(&built.root));
        }
        let same_page = self.stack.visible_child_name().as_deref() == Some("grid");
        *self.grid.borrow_mut() = Some(built);
        self.grid_selected.set(index);
        self.stack.set_visible_child_name("grid");
        if empty {
            self.update_hints();
            return;
        }
        if let Some(tile) = self.grid.borrow().as_ref().and_then(|g| g.tiles.get(index)) {
            tile.add_css_class("selected");
        }
        // A refreshed grid keeps its scroll position; a new one starts at the top.
        let adjustment = self.grid_scroll.vadjustment();
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(ui) = weak.upgrade() {
                adjustment.set_value(if same_page { scroll } else { 0. });
                if !same_page {
                    ui.select_tile(ui.grid_selected.get());
                }
            }
        });
        self.update_hints();
        self.selection_changed();
    }
    fn render_form(self: &Rc<Self>, view: &Value) {
        for field in view["fields"].as_array().into_iter().flatten() {
            let label = field["label"].as_str().unwrap_or("");
            let id = field["id"].as_str().unwrap_or("").to_owned();
            let required = field["required"].as_bool().unwrap_or(false);
            let kind = field["type"].as_str().unwrap_or("text");
            if kind == "description" {
                if !label.is_empty() {
                    let heading = gtk::Label::builder().label(label).xalign(0.).build();
                    heading.add_css_class("field-label");
                    self.detail.append(&heading);
                }
                let text = gtk::Label::builder()
                    .label(field["text"].as_str().unwrap_or(""))
                    .xalign(0.)
                    .wrap(true)
                    .selectable(true)
                    .build();
                text.add_css_class("field-description");
                self.detail.append(&text);
                continue;
            }
            let mut values = Vec::new();
            let widget: gtk::Widget = match kind {
                "boolean" => {
                    let check = gtk::CheckButton::with_label(label);
                    check.set_active(field["value"].as_bool().unwrap_or(false));
                    check.upcast()
                }
                _ => {
                    let label_widget = gtk::Label::builder().label(label).xalign(0.).build();
                    label_widget.add_css_class("field-label");
                    self.detail.append(&label_widget);
                    if kind == "dropdown" {
                        let options = field["options"].as_array().cloned().unwrap_or_default();
                        let titles: Vec<&str> =
                            options.iter().map(|o| o["title"].as_str().unwrap_or("")).collect();
                        values = options
                            .iter()
                            .map(|o| o["value"].as_str().unwrap_or("").to_owned())
                            .collect();
                        let dropdown = gtk::DropDown::from_strings(&titles);
                        dropdown.set_halign(gtk::Align::Start);
                        if let Some(at) = values.iter().position(|v| field["value"] == **v) {
                            dropdown.set_selected(at as u32);
                        }
                        dropdown.upcast()
                    } else {
                        let entry = gtk::Entry::new();
                        entry.set_visibility(kind != "password");
                        entry.set_text(field["value"].as_str().unwrap_or(""));
                        entry.set_placeholder_text(field["placeholder"].as_str());
                        entry.set_tooltip_text(Some(label));
                        // Enter in the last text field submits, as in Raycast.
                        let weak = Rc::downgrade(self);
                        entry.connect_activate(move |_| {
                            if let Some(ui) = weak.upgrade() {
                                ui.submit_form();
                            }
                        });
                        entry.upcast()
                    }
                }
            };
            self.detail.append(&widget);
            if let Some(info) = field["info"].as_str().filter(|i| !i.is_empty()) {
                let info = gtk::Label::builder().label(info).xalign(0.).wrap(true).build();
                info.add_css_class("field-info");
                self.detail.append(&info);
            }
            self.form.borrow_mut().push((id, widget, required, values));
        }
        let submit = gtk::Button::with_label(view["submit_label"].as_str().unwrap_or("Submit"));
        submit.add_css_class("suggested-action");
        submit.add_css_class("pill");
        submit.set_halign(gtk::Align::Start);
        let weak = Rc::downgrade(self);
        submit.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.submit_form();
            }
        });
        self.detail.append(&submit);
        // Focus the first text field so typing goes straight into the form.
        if let Some(entry) = self
            .form
            .borrow()
            .iter()
            .find_map(|f| f.1.downcast_ref::<gtk::Entry>().cloned())
        {
            let weak = entry.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(entry) = weak.upgrade() {
                    entry.grab_focus();
                    entry.set_position(-1);
                }
            });
        }
    }
    fn submit_form(self: &Rc<Self>) {
        let mut values = serde_json::Map::new();
        for (id, widget, required, choices) in self.form.borrow().iter() {
            if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
                if *required && entry.text().trim().is_empty() {
                    self.status.set_label("Complete the required fields");
                    entry.grab_focus();
                    return;
                }
                values.insert(id.clone(), json!(entry.text().as_str()));
            } else if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                values.insert(id.clone(), json!(check.is_active()));
            } else if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
                let value = choices.get(dropdown.selected() as usize).cloned().unwrap_or_default();
                values.insert(id.clone(), json!(value));
            }
        }
        self.extension_request("command.submit", json!({"values":values}));
    }
}

/// Reads a whole clipboard stream, then hands the bytes to `done`.
fn read_stream(stream: gio::InputStream, done: impl FnOnce(Vec<u8>) + 'static) {
    glib::spawn_future_local(async move {
        let mut bytes = Vec::new();
        loop {
            match stream.read_bytes_future(64 * 1024, glib::Priority::DEFAULT).await {
                Ok(chunk) if !chunk.is_empty() && bytes.len() < 4 * 1024 * 1024 => {
                    bytes.extend_from_slice(&chunk)
                }
                _ => break,
            }
        }
        done(bytes);
    });
}

/// Local paths from `text/uri-list` or Files' `x-special/gnome-copied-files`
/// (which starts with a `copy` or `cut` line).
fn clipboard_files(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("file://"))
        .filter_map(|uri| gio::File::for_uri(uri).path())
        .map(|p| p.display().to_string())
        .collect()
}

/// Puts a file on the clipboard as Files would, plus its pixels if it is an
/// image, so it pastes into both file managers and image editors.
fn copy_file(clipboard: &gdk::Clipboard, path: &str) {
    let file = gio::File::for_path(path);
    let uri = file.uri();
    let mut providers = vec![
        gdk::ContentProvider::for_bytes(
            "x-special/gnome-copied-files",
            &glib::Bytes::from_owned(format!("copy\n{uri}").into_bytes()),
        ),
        gdk::ContentProvider::for_bytes(
            "text/uri-list",
            &glib::Bytes::from_owned(format!("{uri}\r\n").into_bytes()),
        ),
    ];
    if let Ok(texture) = gdk::Texture::from_file(&file) {
        providers.insert(0, gdk::ContentProvider::for_value(&texture.to_value()));
    }
    let _ = clipboard.set_content(Some(&gdk::ContentProvider::new_union(&providers)));
}

/// Shows Raycast's HUD: a short message the Shell draws after Lumen closes.
/// Without the Lumen shell extension it becomes a desktop notification.
fn show_hud(app: &adw::Application, text: &str) {
    let text = text.to_owned();
    let app = app.clone();
    gio::bus_get(gio::BusType::Session, None::<&gio::Cancellable>, move |bus| {
        let fallback = {
            let (app, text) = (app.clone(), text.clone());
            move || {
                let notification = gio::Notification::new(&text);
                app.send_notification(None, &notification);
            }
        };
        let Ok(bus) = bus else { return fallback() };
        bus.call(
            Some("org.gnome.Shell"),
            "/io/github/gnomelauncher/Shell",
            "io.github.gnomelauncher.Shell",
            "ShowHud",
            Some(&(text.as_str(), "").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
            move |result| {
                if result.is_err() {
                    fallback();
                }
            },
        );
    });
}

fn section(kind: &Kind) -> &'static str {
    match kind {
        Kind::App(_) => "Applications",
        Kind::File(_) => "Files & folders",
        Kind::Calculation(_) => "Calculator",
        Kind::Command { .. } => "Commands",
        Kind::Settings => "Lumen",
    }
}

/// One result: icon, title and subtitle, and a small kind badge on the right.
fn result_row(item: &Item, favorite: bool, decor: &Value) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gio::Icon::for_string(&item.icon)
        .ok()
        .map(|i| gtk::Image::from_gicon(&i))
        .unwrap_or_else(|| gtk::Image::from_icon_name("application-x-executable-symbolic"));
    // Symbolic icons may also be files an extension ships, named `*-symbolic.svg`.
    if item.icon.ends_with("-symbolic") || item.icon.ends_with("-symbolic.svg") {
        // Symbolic glyphs sit on a soft tile so they line up with full-colour app icons.
        icon.set_pixel_size(16);
        icon.add_css_class("symbolic-tile");
    } else {
        icon.set_pixel_size(32);
    }
    icon.set_valign(gtk::Align::Center);
    content.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let title = gtk::Label::builder()
        .label(&item.title)
        .xalign(0.)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title.add_css_class("result-title");
    if matches!(item.kind, Kind::Calculation(_)) {
        title.add_css_class("calc-value");
    }
    text.append(&title);
    if !item.subtitle.is_empty() {
        let subtitle = gtk::Label::builder()
            .label(&item.subtitle)
            .xalign(0.)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        subtitle.add_css_class("result-subtitle");
        text.append(&subtitle);
    }
    content.append(&text);
    if favorite {
        let star = gtk::Image::from_icon_name("starred-symbolic");
        star.add_css_class("favorite");
        star.set_valign(gtk::Align::Center);
        content.append(&star);
    }
    let kind = match &item.kind {
        Kind::App(_) => "Application",
        Kind::File(_) if item.icon == "folder" => "Folder",
        Kind::File(_) => "File",
        Kind::Calculation(_) => "Calculator",
        Kind::Command { extension, .. } if extension.is_empty() => "",
        Kind::Command { .. } => "Command",
        Kind::Settings => "Lumen",
    };
    if !kind.is_empty() {
        let badge = gtk::Label::new(Some(kind));
        badge.add_css_class("result-kind");
        badge.set_valign(gtk::Align::Center);
        content.append(&badge);
    }
    if !decor.is_null() {
        rich::decorate(&icon, &text, &content, decor);
    }
    content
}

fn reveal_in_files(uri: String) {
    gio::bus_get(
        gio::BusType::Session,
        None::<&gio::Cancellable>,
        move |result| {
            if let Ok(bus) = result {
                bus.call(
                    Some("org.freedesktop.FileManager1"),
                    "/org/freedesktop/FileManager1",
                    "org.freedesktop.FileManager1",
                    "ShowItems",
                    Some(&(vec![uri], "").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                    None::<&gio::Cancellable>,
                    |r| {
                        if let Err(e) = r {
                            eprintln!("Reveal: {e}");
                        }
                    },
                );
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spin_until(check: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !check() {
            while context.pending() {
                context.iteration(false);
            }
            assert!(Instant::now() < deadline, "UI operation timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "requires a GNOME display; run isolated with --test-threads=1"]
    fn native_palette() {
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.gnomelauncher.Test")
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Ui::new(&app);
        ui.show();
        spin_until(|| ui.window.is_mapped());
        assert!(!ui.results.borrow().is_empty());
        assert!(ui.list.row_at_index(0).unwrap().header().is_some());
        ui.entry.set_text("2 + 3 * 4");
        assert!(matches!(&ui.results.borrow()[0].kind, Kind::Calculation(v) if v == "14"));
        ui.entry.set_text("Files");
        assert!(
            ui.results
                .borrow()
                .iter()
                .any(|i| matches!(i.kind, Kind::App(_)))
        );
        // A stale response must not overwrite the new query or keep polling alive.
        ui.entry.set_text("png");
        ui.entry.set_text("nonexistent-application-qzxq");
        assert!(ui.results.borrow().is_empty());
        ui.back();
        ui.open_settings();
        assert!(ui.settings_window.borrow().as_ref().unwrap().is_visible());
        ui.settings_window.borrow().as_ref().unwrap().close();
        let ext = package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links"),
        )
        .unwrap();
        *ui.active.borrow_mut() = Some((ext, "links".into()));
        ui.extension_request("command.run", json!({}));
        spin_until(|| ui.view.borrow()["type"] == "list");
        assert_eq!(ui.results.borrow().len(), 3);
        ui.back();
        let mut ext = package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scratchpad"),
        )
        .unwrap();
        ext.manifest.id = format!("lumen-smoke-{}", std::process::id());
        let storage = store::data_dir()
            .join("extension-storage")
            .join(format!("{}.json", ext.manifest.id));
        *ui.active.borrow_mut() = Some((ext, "note".into()));
        ui.extension_request("command.run", json!({}));
        spin_until(|| ui.view.borrow()["type"] == "form");
        assert_eq!(ui.form.borrow().len(), 2);
        let entry = ui.form.borrow()[0]
            .1
            .clone()
            .downcast::<gtk::Entry>()
            .unwrap();
        entry.set_text("Native form smoke test");
        let check = ui.form.borrow()[1]
            .1
            .clone()
            .downcast::<gtk::CheckButton>()
            .unwrap();
        check.set_active(false);
        ui.detail
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        spin_until(|| ui.view.borrow()["type"] == "detail");
        assert_eq!(ui.view.borrow()["text"], "Native form smoke test");
        let _ = std::fs::remove_file(storage);
        ui.back();
        // Native GTK renders, useful for visual review without capturing other windows.
        if let Some(dir) = std::env::var_os("LUMEN_SCREENSHOT") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            let settle = |ms| {
                let context = glib::MainContext::default();
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    while context.pending() {
                        context.iteration(false);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            let capture = |name: &str| {
                settle(400);
                let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(
                    &snapshot,
                    ui.window.width() as f64,
                    ui.window.height() as f64,
                );
                let node = snapshot.to_node().unwrap();
                let texture = ui.window.renderer().unwrap().render_texture(&node, None);
                texture
                    .save_to_png(dir.join(format!("{name}.png")))
                    .unwrap();
            };
            let styles = adw::StyleManager::default();
            for (scheme, suffix) in [
                (adw::ColorScheme::ForceLight, "light"),
                (adw::ColorScheme::ForceDark, "dark"),
            ] {
                styles.set_color_scheme(scheme);
                ui.back();
                capture(&format!("lumen-suggestions-{suffix}"));
                let query = std::env::var("LUMEN_SCREENSHOT_QUERY").unwrap_or("te".into());
                ui.entry.set_text(&query);
                ui.move_selection(1);
                capture(&format!("lumen-search-{suffix}"));
                ui.entry.set_text("sqrt(144) * 3");
                capture(&format!("lumen-calculator-{suffix}"));
                ui.entry.set_text("qzxqv nothing");
                capture(&format!("lumen-empty-{suffix}"));
                ui.entry.set_text("Files");
                spin_until(|| !ui.file_results.borrow().is_empty());
                capture(&format!("lumen-files-{suffix}"));
            }
            styles.set_color_scheme(adw::ColorScheme::Default);
        }
        ui.hide();
        ui.window.destroy();
    }
    /// Renders saved extension views for visual review: every `*.json` view in
    /// LUMEN_VIEW_DIR becomes `<name>-light.png` and `<name>-dark.png` in
    /// LUMEN_SCREENSHOT. LUMEN_VIEW_SELECT picks rows, as in `ports-1=2,speed-1=3`.
    /// A view's optional `_scope` names the command in the search bar chip, and
    /// `_query` fills the search field.
    #[test]
    #[ignore = "requires a GNOME display and LUMEN_VIEW_DIR; run isolated with --test-threads=1"]
    fn extension_view_screenshots() {
        let (Some(views), Some(out)) = (
            std::env::var_os("LUMEN_VIEW_DIR"),
            std::env::var_os("LUMEN_SCREENSHOT"),
        ) else {
            return;
        };
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.gnomelauncher.ViewTest")
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Ui::new(&app);
        ui.show();
        spin_until(|| ui.window.is_mapped());
        let selections: std::collections::HashMap<String, i32> = std::env::var("LUMEN_VIEW_SELECT")
            .unwrap_or_default()
            .split(',')
            .filter_map(|pair| pair.split_once('='))
            .filter_map(|(name, row)| Some((name.to_owned(), row.parse().ok()?)))
            .collect();
        let ext = package::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quick-links"),
        )
        .unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&views)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let settle = |ms| {
            let context = glib::MainContext::default();
            let until = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < until {
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let styles = adw::StyleManager::default();
        for (scheme, suffix) in [
            (adw::ColorScheme::ForceLight, "light"),
            (adw::ColorScheme::ForceDark, "dark"),
        ] {
            styles.set_color_scheme(scheme);
            for file in &files {
                let name = file.file_stem().unwrap().to_string_lossy().to_string();
                let view: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
                crate::extensions::runtime::validate_view(&view).unwrap();
                ui.back();
                *ui.active.borrow_mut() = Some((ext.clone(), "links".into()));
                ui.scope.set_visible(true);
                let scope = view["_scope"].as_str().unwrap_or(&name).to_owned();
                ui.scope.set_child(Some(&{
                    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    content.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
                    content.append(&gtk::Label::new(Some(&scope)));
                    content
                }));
                ui.status.set_label("Esc to go back");
                ui.suppress_search.set(true);
                ui.entry.set_text(view["_query"].as_str().unwrap_or(""));
                ui.suppress_search.set(false);
                ui.render_view(view);
                if let Some(row) = ui.list.row_at_index(*selections.get(&name).unwrap_or(&0)) {
                    ui.list.select_row(Some(&row));
                }
                settle(400);
                let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(&snapshot, ui.window.width() as f64, ui.window.height() as f64);
                let node = snapshot.to_node().unwrap();
                let texture = ui.window.renderer().unwrap().render_texture(&node, None);
                texture.save_to_png(out.join(format!("{name}-{suffix}.png"))).unwrap();
            }
        }
        ui.hide();
        ui.window.destroy();
    }
}
