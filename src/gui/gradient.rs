//! Multi-stop color gradients for box-drawing borders (Aurora, Nebula, Rainbow, ...).
//!
//! Stops are interpolated in OKLab so mid-tones stay vivid instead of turning
//! muddy as they do in RGB. Borders are painted along a top-left to
//! bottom-right diagonal, which has no seam where a loop around the frame
//! would otherwise jump from the last stop back to the first.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

pub type Rgb = (u8, u8, u8);

/// Named palettes, shared with notMyShell's Chroma presets.
const PRESETS: &[(&str, &[&str])] = &[
    ("lavender", &["#a67cf3", "#c5a4fa", "#9979d9"]),
    ("aurora", &["#3ee8b5", "#45b8f0", "#8f6cf5", "#e46cc8"]),
    (
        "rainbow",
        &[
            "#ff5f6d", "#ffa647", "#f2e35b", "#5be584", "#4fb8f7", "#9575f0",
        ],
    ),
    ("nebula", &["#7b2fbf", "#c2389e", "#5a4ff0", "#3fc2f0"]),
    ("blackhole", &["#3d1f6e", "#7a2a8c", "#e0566b", "#ffb36b"]),
    ("warm", &["#e8573c", "#f29e4c", "#f1c453", "#d9486b"]),
    ("cool", &["#2ec4b6", "#3a86ff", "#5e60ce", "#48bfe3"]),
    ("monochrome", &["#6a6e78", "#d4d7dd", "#f4f5f7", "#9a9ea8"]),
];

/// Parses a preset name or a list of two or more `#rrggbb` stops separated by
/// commas or spaces. Empty, `off` and `none` disable the gradient.
pub fn parse_stops(spec: &str) -> Option<Vec<Rgb>> {
    let spec = spec.trim();
    if spec.is_empty() || spec.eq_ignore_ascii_case("off") || spec.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some((_, hexes)) = PRESETS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(spec))
    {
        return hexes.iter().map(|hex| parse_hex(hex)).collect();
    }
    let stops: Vec<Rgb> = spec
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(parse_hex)
        .collect::<Option<_>>()?;
    (stops.len() >= 2).then_some(stops)
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f32) -> u8 {
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn to_oklab((r, g, b): Rgb) -> [f32; 3] {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> Rgb {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    (
        from_linear(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        from_linear(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        from_linear(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    )
}

/// The gradient color at `t` (0.0 = first stop, 1.0 = last stop).
pub fn sample(stops: &[Rgb], t: f32) -> Rgb {
    match stops {
        [] => (0, 0, 0),
        [only] => *only,
        _ => {
            let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
            let i = (x as usize).min(stops.len() - 2);
            let f = x - i as f32;
            let (p, q) = (to_oklab(stops[i]), to_oklab(stops[i + 1]));
            from_oklab([
                p[0] + (q[0] - p[0]) * f,
                p[1] + (q[1] - p[1]) * f,
                p[2] + (q[2] - p[2]) * f,
            ])
        }
    }
}

fn is_box_drawing(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if ('\u{2500}'..='\u{257F}').contains(&c))
}

/// Diagonal position of `(x, y)` inside `area`: 0.0 top-left, 1.0 bottom-right.
fn diagonal(area: Rect, x: u16, y: u16) -> f32 {
    let span = |offset: u16, len: u16| {
        if len <= 1 {
            0.0
        } else {
            offset as f32 / (len - 1) as f32
        }
    };
    (span(x - area.x, area.width) + span(y - area.y, area.height)) / 2.0
}

/// Recolors every box-drawing cell drawn in `target` (the active border
/// color) along one diagonal spanning their bounding box, so the focused
/// panel and any popup above it share a single continuous gradient. Titles
/// and other text keep their colors; modifiers such as bold are preserved.
pub fn paint_matching(buf: &mut Buffer, target: Color, stops: &[Rgb]) {
    let area = buf.area;
    let mut cells = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            if cell.fg == target && is_box_drawing(cell.symbol()) {
                cells.push((x, y));
            }
        }
    }
    let Some(&(first_x, first_y)) = cells.first() else {
        return;
    };
    let (mut left, mut top, mut right, mut bottom) = (first_x, first_y, first_x, first_y);
    for &(x, y) in &cells {
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    let bounds = Rect::new(left, top, right - left + 1, bottom - top + 1);
    for (x, y) in cells {
        let (r, g, b) = sample(stops, diagonal(bounds, x, y));
        buf[(x, y)].set_fg(Color::Rgb(r, g, b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::{Block, BorderType, Widget};

    #[test]
    fn parses_presets_custom_lists_and_off() {
        assert_eq!(parse_stops("Aurora").unwrap().len(), 4);
        assert_eq!(
            parse_stops("#ff0000, #0000ff").unwrap(),
            vec![(255, 0, 0), (0, 0, 255)]
        );
        assert!(parse_stops("").is_none());
        assert!(parse_stops("off").is_none());
        assert!(
            parse_stops("#ff0000").is_none(),
            "a single stop is not a gradient"
        );
        assert!(parse_stops("#ff0000,nope").is_none());
    }

    #[test]
    fn sample_hits_stops_exactly_at_the_ends() {
        let stops = parse_stops("nebula").unwrap();
        assert_eq!(sample(&stops, 0.0), stops[0]);
        assert_eq!(sample(&stops, 1.0), stops[3]);
        assert_eq!(sample(&stops, 1.0 / 3.0), stops[1]);
    }

    #[test]
    fn paints_only_border_cells_in_the_target_color() {
        let area = Rect::new(0, 0, 12, 5);
        let mut buf = Buffer::empty(area);
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Color::Green)
            .title("Files")
            .render(area, &mut buf);
        buf[(5, 2)].set_symbol("─").set_fg(Color::Blue);

        let stops = vec![(255, 0, 0), (0, 0, 255)];
        paint_matching(&mut buf, Color::Green, &stops);

        assert_eq!(
            buf[(0, 0)].fg,
            Color::Rgb(255, 0, 0),
            "top-left starts the gradient"
        );
        assert_eq!(
            buf[(11, 4)].fg,
            Color::Rgb(0, 0, 255),
            "bottom-right ends it"
        );
        assert_eq!(buf[(1, 0)].symbol(), "F");
        assert_eq!(
            buf[(1, 0)].fg,
            Color::Green,
            "title text keeps the border color it inherited"
        );
        assert_eq!(buf[(5, 2)].fg, Color::Blue, "other colors are left alone");
    }
}
