//! Native rendering for the richer parts of extension list views: tinted
//! icons, accessories and tags, row progress, and the detail side pane.
//! Extensions describe these as plain JSON; nothing here runs extension code.
use gtk::prelude::*;
use serde_json::Value;

/// Colour names extensions may use; each maps to a `tint-<name>` CSS class.
pub const COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "blue", "purple", "accent", "dim",
];

pub(super) fn tint(widget: &impl IsA<gtk::Widget>, color: &Value) {
    if let Some(color) = color.as_str().filter(|c| COLORS.contains(c)) {
        widget.add_css_class(&format!("tint-{color}"));
    }
}

/// An icon name, or the path of an image an extension ships (symbolic when
/// named `*-symbolic.svg`, so it follows the text colour and tints).
pub(super) fn icon(name: &str) -> gtk::Image {
    match name
        .starts_with('/')
        .then(|| gtk::gio::Icon::for_string(name).ok())
        .flatten()
    {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name(name),
    }
}

/// Tags without a colour get a neutral look.
fn tag_tint(label: &gtk::Label, color: &Value) {
    if color.as_str().is_some_and(|c| COLORS.contains(&c)) {
        tint(label, color);
    } else {
        label.add_css_class("plain");
    }
}

/// Adds an extension row's tint, progress bar and accessories to the row
/// that `result_row` built. `text` is the title/subtitle column.
pub fn decorate(icon: &gtk::Image, text: &gtk::Box, content: &gtk::Box, item: &Value) {
    tint(icon, &item["tint"]);
    // Accessories never squeeze the title below a readable width.
    if item["accessories"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        text.set_size_request(80, -1);
    }
    if let Some(progress) = item["progress"].as_f64() {
        let bar = gtk::ProgressBar::builder()
            .fraction(progress.clamp(0., 1.))
            .valign(gtk::Align::Center)
            .build();
        bar.add_css_class("row-progress");
        tint(&bar, &item["tint"]);
        text.append(&bar);
    }
    for accessory in item["accessories"].as_array().into_iter().flatten().take(6) {
        content.append(&accessory_widget(accessory));
    }
}

fn accessory_widget(accessory: &Value) -> gtk::Widget {
    let tooltip = accessory["tooltip"].as_str();
    if let Some(tag) = accessory["tag"].as_str() {
        let label = gtk::Label::new(Some(tag));
        label.add_css_class("tag");
        tag_tint(&label, &accessory["color"]);
        label.set_valign(gtk::Align::Center);
        label.set_tooltip_text(tooltip);
        return label.upcast();
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("accessory");
    row.set_valign(gtk::Align::Center);
    if let Some(name) = accessory["icon"].as_str() {
        let image = icon(name);
        image.set_pixel_size(14);
        tint(&image, &accessory["color"]);
        row.append(&image);
    }
    if let Some(text) = accessory["text"].as_str() {
        let label = gtk::Label::new(Some(text));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(24);
        tint(&label, &accessory["color"]);
        row.append(&label);
    }
    row.set_tooltip_text(tooltip);
    row.upcast()
}

/// Fills the side pane with the selected item's `detail`.
pub fn fill_detail(pane: &gtk::Box, item: &Value) {
    while let Some(child) = pane.first_child() {
        pane.remove(&child);
    }
    let detail = &item["detail"];
    let title = gtk::Label::builder()
        .label(item["title"].as_str().unwrap_or(""))
        .xalign(0.)
        .wrap(true)
        .build();
    title.add_css_class("side-title");
    pane.append(&title);
    if let Some(path) = detail["image"].as_str() {
        pane.append(&picture(path, 248, 186));
    }
    let hero = &detail["hero"];
    if let Some(value) = hero["value"].as_str() {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let number = gtk::Label::new(Some(value));
        number.add_css_class("hero-value");
        tint(&number, &hero["color"]);
        number.set_valign(gtk::Align::Baseline);
        line.append(&number);
        if let Some(unit) = hero["unit"].as_str() {
            let unit = gtk::Label::new(Some(unit));
            unit.add_css_class("hero-unit");
            unit.set_valign(gtk::Align::Baseline);
            line.append(&unit);
        }
        pane.append(&line);
        if let Some(progress) = hero["progress"].as_f64() {
            let bar = gtk::ProgressBar::builder()
                .fraction(progress.clamp(0., 1.))
                .build();
            bar.add_css_class("hero-progress");
            tint(&bar, &hero["color"]);
            pane.append(&bar);
        }
        if let Some(caption) = hero["caption"].as_str() {
            let caption = gtk::Label::builder()
                .label(caption)
                .xalign(0.)
                .wrap(true)
                .build();
            caption.add_css_class("hero-caption");
            pane.append(&caption);
        }
    }
    let chart = &detail["chart"];
    let values: Vec<f64> = chart["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .filter(|v| v.is_finite())
        .take(200)
        .collect();
    if values.len() >= 2 {
        pane.append(&sparkline(values, &chart["color"]));
        if let Some(caption) = chart["caption"].as_str() {
            let caption = gtk::Label::builder().label(caption).xalign(0.).build();
            caption.add_css_class("hero-caption");
            pane.append(&caption);
        }
    }
    if let Some(text) = detail["text"].as_str() {
        let body = gtk::Label::builder()
            .label(text)
            .xalign(0.)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .selectable(true)
            .build();
        body.add_css_class("side-text");
        pane.append(&body);
    }
    if let Some(grid) = metadata_grid(&detail["metadata"]) {
        pane.append(&grid);
    }
}

/// A local image scaled to fit `width`×`height`, keeping its aspect ratio.
pub(super) fn picture(path: &str, width: i32, height: i32) -> gtk::Picture {
    let picture = gtk::Picture::for_filename(path);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_size_request(width, height);
    picture.set_halign(gtk::Align::Center);
    picture.add_css_class("detail-image");
    picture
}

/// Label/value rows, separators and tag rows, as Raycast's metadata.
pub(super) fn metadata_grid(metadata: &Value) -> Option<gtk::Grid> {
    let rows = metadata.as_array().into_iter().flatten().take(40);
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(6)
        .build();
    grid.add_css_class("metadata");
    let mut line = 0;
    for row in rows {
        if row["separator"].as_bool() == Some(true) {
            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            separator.set_margin_top(4);
            separator.set_margin_bottom(4);
            grid.attach(&separator, 0, line, 2, 1);
            line += 1;
            continue;
        }
        let label = gtk::Label::builder()
            .label(row["label"].as_str().unwrap_or(""))
            .xalign(0.)
            .yalign(0.)
            .build();
        label.add_css_class("metadata-label");
        grid.attach(&label, 0, line, 1, 1);
        let value: gtk::Widget = if let Some(tags) = row["tags"].as_array() {
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .max_children_per_line(6)
                .column_spacing(4)
                .row_spacing(4)
                .hexpand(true)
                .build();
            for tag in tags.iter().take(20) {
                let chip = gtk::Label::new(tag["text"].as_str());
                chip.add_css_class("tag");
                tag_tint(&chip, &tag["color"]);
                chip.set_halign(gtk::Align::Start);
                flow.insert(&chip, -1);
            }
            flow.upcast()
        } else {
            let text = gtk::Label::builder()
                .label(row["text"].as_str().unwrap_or("—"))
                .xalign(0.)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .hexpand(true)
                .build();
            text.add_css_class("metadata-value");
            tint(&text, &row["color"]);
            text.upcast()
        };
        grid.attach(&value, 1, line, 1, 1);
        line += 1;
    }
    (line > 0).then_some(grid)
}

/// A small line chart of recent samples, drawn in the widget's CSS colour.
fn sparkline(values: Vec<f64>, color: &Value) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(56)
        .hexpand(true)
        .build();
    area.add_css_class("sparkline");
    tint(&area, color);
    area.set_draw_func(move |area, cr, width, height| {
        let (width, height) = (width as f64, height as f64 - 4.);
        let max = values.iter().cloned().fold(f64::MIN, f64::max);
        let min = values.iter().cloned().fold(f64::MAX, f64::min).min(0.);
        let span = (max - min).max(f64::EPSILON);
        let step = width / (values.len() - 1) as f64;
        let point = |i: usize, v: f64| (i as f64 * step, 2. + height - (v - min) / span * height);
        let rgba = area.color();
        let (r, g, b) = (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);
        cr.move_to(0., 2. + height);
        for (i, v) in values.iter().enumerate() {
            let (x, y) = point(i, *v);
            cr.line_to(x, y);
        }
        cr.line_to(width, 2. + height);
        cr.close_path();
        cr.set_source_rgba(r, g, b, 0.15);
        let _ = cr.fill();
        for (i, v) in values.iter().enumerate() {
            let (x, y) = point(i, *v);
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        cr.set_source_rgba(r, g, b, 0.9);
        cr.set_line_width(2.);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        let _ = cr.stroke();
    });
    area
}
