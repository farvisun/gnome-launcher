//! Raycast-style grid views: tiles with a picture or an icon, grouped in
//! sections. Selection is tracked by index so the arrow keys can move in two
//! dimensions while the search field keeps focus.
use gtk::prelude::*;
use serde_json::Value;

/// Width of the grid's content: the 720 px window minus the side padding.
const WIDTH: i32 = 692;
const SPACING: i32 = 10;
/// Horizontal padding of a tile button (`button.grid-tile` in style.css), both sides.
const TILE_PADDING: i32 = 10;

pub struct Grid {
    pub root: gtk::Box,
    pub tiles: Vec<gtk::Button>,
    /// `(first index, length)` of each section, in order.
    pub sections: Vec<(usize, usize)>,
    pub columns: usize,
}

fn aspect(view: &Value) -> f64 {
    match view["aspect"].as_str().unwrap_or("1") {
        "3/2" => 1.5,
        "2/3" => 2. / 3.,
        "4/3" => 4. / 3.,
        "3/4" => 0.75,
        "16/9" => 16. / 9.,
        "9/16" => 9. / 16.,
        _ => 1.,
    }
}

/// Builds the tiles; `activate` receives a tile's index when it is clicked.
pub fn build(view: &Value, activate: impl Fn(usize) + Clone + 'static) -> Grid {
    let columns = view["columns"].as_u64().unwrap_or(5).clamp(3, 8) as usize;
    // Each tile's button adds its padding around the frame.
    let width = (WIDTH - SPACING * (columns as i32 - 1)) / columns as i32 - TILE_PADDING;
    let height = (f64::from(width) / aspect(view)).round() as i32;
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("grid-view");
    let mut tiles = Vec::new();
    let mut sections: Vec<(usize, usize)> = Vec::new();
    let mut current: Option<(String, gtk::Grid)> = None;
    for (index, item) in view["items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(100)
        .enumerate()
    {
        let name = item["section"].as_str().unwrap_or("").to_owned();
        if current.as_ref().is_none_or(|(n, _)| *n != name) {
            if !name.is_empty() {
                let label = gtk::Label::builder().label(&name).xalign(0.).build();
                label.add_css_class("section-label");
                root.append(&label);
            }
            let grid = gtk::Grid::builder()
                .column_spacing(SPACING)
                .row_spacing(SPACING)
                .column_homogeneous(true)
                .build();
            root.append(&grid);
            current = Some((name, grid));
            sections.push((index, 0));
        }
        let (_, grid) = current.as_ref().unwrap();
        let section = sections.last_mut().unwrap();
        let position = section.1 as i32;
        section.1 += 1;
        let tile = tile(item, width, height);
        let activate = activate.clone();
        tile.connect_clicked(move |_| activate(index));
        grid.attach(
            &tile,
            position % columns as i32,
            position / columns as i32,
            1,
            1,
        );
        tiles.push(tile);
    }
    Grid {
        root,
        tiles,
        sections,
        columns,
    }
}

fn tile(item: &Value, width: i32, height: i32) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("grid-frame");
    frame.set_size_request(width, height);
    frame.set_overflow(gtk::Overflow::Hidden);
    let media = &item["content"];
    if let Some(path) = media["image"].as_str() {
        let picture = gtk::Picture::for_filename(path);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture.set_vexpand(true);
        frame.append(&picture);
    } else {
        let icon = super::rich::icon(media["icon"].as_str().unwrap_or("image-x-generic-symbolic"));
        icon.set_pixel_size((height / 3).clamp(16, 48));
        icon.set_vexpand(true);
        icon.set_valign(gtk::Align::Center);
        super::rich::tint(&icon, &media["tint"]);
        frame.append(&icon);
    }
    content.append(&frame);
    let title = item["title"].as_str().unwrap_or("");
    if !title.is_empty() {
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .build();
        label.add_css_class("grid-title");
        content.append(&label);
    }
    if let Some(subtitle) = item["subtitle"].as_str().filter(|s| !s.is_empty()) {
        let label = gtk::Label::builder()
            .label(subtitle)
            .xalign(0.)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .build();
        label.add_css_class("grid-subtitle");
        content.append(&label);
    }
    let button = gtk::Button::builder()
        .child(&content)
        .focusable(false)
        .build();
    button.add_css_class("flat");
    button.add_css_class("grid-tile");
    button.set_tooltip_text(
        item["tooltip"]
            .as_str()
            .or(Some(title))
            .filter(|t| !t.is_empty()),
    );
    button
}

/// The tile the arrow keys move to from `index`. Up and down keep the column,
/// crossing into the neighbouring section at its nearest row.
pub fn neighbour(
    sections: &[(usize, usize)],
    columns: usize,
    index: usize,
    dx: i32,
    dy: i32,
) -> usize {
    let total: usize = sections.iter().map(|s| s.1).sum();
    if total == 0 {
        return 0;
    }
    if dx != 0 {
        return (index as i64 + i64::from(dx)).clamp(0, total as i64 - 1) as usize;
    }
    let Some(at) = sections
        .iter()
        .position(|&(start, len)| index >= start && index < start + len)
    else {
        return index.min(total - 1);
    };
    let (start, len) = sections[at];
    let position = index - start;
    let column = position % columns;
    let row = (position / columns) as i64 + i64::from(dy);
    let rows = len.div_ceil(columns) as i64;
    if (0..rows).contains(&row) {
        return start + (row as usize * columns + column).min(len - 1);
    }
    if dy > 0 && at + 1 < sections.len() {
        let (start, len) = sections[at + 1];
        return start + column.min(len - 1);
    }
    if dy < 0 && at > 0 {
        let (start, len) = sections[at - 1];
        let last_row = (len - 1) / columns * columns;
        return start + (last_row + column).min(len - 1);
    }
    index
}

#[cfg(test)]
mod tests {
    use super::neighbour;
    #[test]
    fn arrows_move_in_two_dimensions_across_sections() {
        // Two sections: 7 tiles (rows of 5 and 2), then 3 tiles.
        let sections = [(0, 7), (7, 3)];
        assert_eq!(neighbour(&sections, 5, 0, 1, 0), 1);
        assert_eq!(neighbour(&sections, 5, 0, -1, 0), 0);
        assert_eq!(neighbour(&sections, 5, 1, 0, 1), 6);
        // Column 4 has no tile in the short second row: take the last one.
        assert_eq!(neighbour(&sections, 5, 4, 0, 1), 6);
        // Down from the last row enters the next section at the same column.
        assert_eq!(neighbour(&sections, 5, 6, 0, 1), 8);
        assert_eq!(neighbour(&sections, 5, 9, 0, -1), 6);
        assert_eq!(neighbour(&sections, 5, 7, 0, -1), 5);
        assert_eq!(neighbour(&sections, 5, 9, 0, 1), 9);
        assert_eq!(neighbour(&sections, 5, 2, 0, -1), 2);
    }
}
