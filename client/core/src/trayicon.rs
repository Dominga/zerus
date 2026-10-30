//! Qt's menu-bar tile (`tray/src/IconFactory.cpp:114-159`) as geometry and
//! colours, and the Zerus mark (`IconFactory.cpp:21-55`). Pure: the platform
//! glue fills the shapes, draws the two counts and reads the pixels back.
use crate::menubar::Face;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// HSL lightness, as `QColor::lightnessF`.
    pub fn lightness(self) -> f64 {
        let max = f64::from(self.r.max(self.g).max(self.b));
        let min = f64::from(self.r.min(self.g).min(self.b));
        (max + min) / 2.0 / 255.0
    }
}

/// A rectangle in pixels, origin at the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// `QRectF::adjusted`.
    pub fn adjusted(self, left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self {
            x: self.x + left,
            y: self.y + top,
            w: self.w - left + right,
            h: self.h - top + bottom,
        }
    }
}

/// A count and the box it is centred in.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub rect: Rect,
    pub color: Rgb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub size: f64,
    pub upper: Rect,
    pub lower: Rect,
    pub radius: f64,
    pub surface: Rgb,
    pub lower_fill: Rgb,
    /// The Zerus mark's box beside a total of at most two characters.
    pub mark: Option<Rect>,
    pub foreground: Rgb,
    pub total: Text,
    pub attention: Text,
    /// The font size tried first; the glue shrinks it until the count fits.
    pub font_pixels: f64,
    /// The stale dot: centre x, centre y, radius.
    pub dot: Option<(f64, f64, f64)>,
}

/// The counter surface behind light text (a dark menu bar) and dark text.
pub const SURFACE_FOR_LIGHT_TEXT: Rgb = Rgb::new(0x29, 0x32, 0x3c);
pub const SURFACE_FOR_DARK_TEXT: Rgb = Rgb::new(0xe3, 0xe9, 0xef);
pub const AMBER: Rgb = Rgb::new(0xf3, 0xbd, 0x64);
pub const ON_AMBER: Rgb = Rgb::new(0x20, 0x25, 0x2c);

/// `IconFactory::renderTile` at `size` pixels.
pub fn tile(face: &Face, size: f64, foreground: Rgb) -> Tile {
    let gap = (size / 22.0).round().max(1.0);
    let upper_height = ((size - gap) / 2.0).floor();
    let upper = Rect {
        x: 0.0,
        y: 0.0,
        w: size,
        h: upper_height,
    };
    let lower = Rect {
        x: 0.0,
        y: upper_height + gap,
        w: size,
        h: size - upper_height - gap,
    };
    let surface = if foreground.lightness() > 0.5 {
        SURFACE_FOR_LIGHT_TEXT
    } else {
        SURFACE_FOR_DARK_TEXT
    };
    // Large capped counts take the whole top row (`IconFactory.cpp:144-146`).
    let mark_size = if face.total.chars().count() <= 2 {
        upper_height
    } else {
        0.0
    };
    let inset = (size / 24.0).round().max(1.0);
    let total_left = if mark_size > 0.0 { mark_size } else { inset };
    Tile {
        size,
        upper,
        lower,
        radius: (size / 9.0).max(1.5),
        surface,
        lower_fill: if face.highlighted { AMBER } else { surface },
        mark: (mark_size > 0.0).then_some(Rect {
            x: 0.0,
            y: 0.0,
            w: mark_size,
            h: mark_size,
        }),
        foreground,
        total: Text {
            text: face.total.clone(),
            rect: upper.adjusted(total_left, 1.0, -inset, -1.0),
            color: foreground,
        },
        attention: Text {
            text: face.attention.clone(),
            rect: lower.adjusted(inset, 1.0, -inset, -1.0),
            color: if face.highlighted {
                ON_AMBER
            } else {
                Rgb {
                    a: 160,
                    ..foreground
                }
            },
        },
        font_pixels: (size * 0.55).round().max(8.0),
        dot: face.dot.then(|| {
            let radius = (size / 16.0).max(1.0);
            (size - radius, radius, radius)
        }),
    }
}

/// One step of a path in a 100-unit box, y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Move(f64, f64),
    Curve {
        c1: (f64, f64),
        c2: (f64, f64),
        to: (f64, f64),
    },
    Close,
}

/// The selected monochrome Swarm: three separated blades with the curves and
/// transforms of `zerusMark` (`IconFactory.cpp:21-55`).
pub fn mark_path() -> Vec<Step> {
    const START: (f64, f64) = (39.0, 9.0);
    const CURVES: [[f64; 6]; 8] = [
        [54.0, 3.0, 70.0, 13.0, 77.0, 29.0],
        [72.0, 25.0, 69.0, 24.0, 72.0, 32.0],
        [81.0, 44.0, 77.0, 58.0, 65.0, 65.0],
        [70.0, 51.0, 65.0, 36.0, 52.0, 28.0],
        [40.0, 21.0, 30.0, 25.0, 22.0, 34.0],
        [25.0, 21.0, 38.0, 15.0, 51.0, 19.0],
        [57.0, 21.0, 56.0, 16.0, 51.0, 13.0],
        [47.0, 11.0, 43.0, 10.0, 39.0, 9.0],
    ];
    // translate(56, 30) scale(0.88) translate(-56, -30)
    let spacing = |(x, y): (f64, f64)| (56.0 + 0.88 * (x - 56.0), 30.0 + 0.88 * (y - 30.0));
    let mut steps = Vec::with_capacity(30);
    for angle in [0.0_f64, 120.0, 240.0] {
        let (sin, cos) = angle.to_radians().sin_cos();
        // translate(50, 50) scale(1.06) rotate(angle) translate(-50, -50); QTransform::rotate turns clockwise with y down.
        let place = |point: (f64, f64)| {
            let (x, y) = spacing(point);
            let (dx, dy) = (x - 50.0, y - 50.0);
            (
                50.0 + 1.06 * (dx * cos - dy * sin),
                50.0 + 1.06 * (dx * sin + dy * cos),
            )
        };
        let (x, y) = place(START);
        steps.push(Step::Move(x, y));
        for curve in CURVES {
            steps.push(Step::Curve {
                c1: place((curve[0], curve[1])),
                c2: place((curve[2], curve[3])),
                to: place((curve[4], curve[5])),
            });
        }
        steps.push(Step::Close);
    }
    steps
}

/// AppKit bitmaps are premultiplied; a tray image is not.
pub fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

/// `#rgb` or `#rrggbb`, the forms `icon-color` documents (`AppConfig.cpp:296-304`).
pub fn parse_color(value: &str) -> Option<Rgb> {
    let hex: Vec<u8> = value
        .trim()
        .strip_prefix('#')?
        .chars()
        .map(|c| c.to_digit(16).map(|digit| digit as u8))
        .collect::<Option<_>>()?;
    match hex.as_slice() {
        [r, g, b] => Some(Rgb::new(r * 17, g * 17, b * 17)),
        [r1, r2, g1, g2, b1, b2] => Some(Rgb::new(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
        _ => None,
    }
}

/// `IconFactory::foreground`: `icon-color`, else white on a dark menu bar
/// and black on a light one.
pub fn foreground(dark: bool, configured: Option<Rgb>) -> Rgb {
    configured.unwrap_or(if dark {
        Rgb::new(255, 255, 255)
    } else {
        Rgb::new(0, 0, 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menubar::Face;

    #[test]
    fn the_tile_follows_qt_geometry_at_36_pixels() {
        let white = Rgb::new(255, 255, 255);
        let tile = tile(&Face::counts(12, 3, true), 36.0, white);
        assert_eq!(
            tile.upper,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 36.0,
                h: 17.0
            }
        );
        assert_eq!(
            tile.lower,
            Rect {
                x: 0.0,
                y: 19.0,
                w: 36.0,
                h: 17.0
            }
        );
        assert_eq!(tile.radius, 4.0);
        assert_eq!(tile.surface, SURFACE_FOR_LIGHT_TEXT);
        assert_eq!(tile.lower_fill, AMBER);
        assert_eq!(
            tile.mark,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                w: 17.0,
                h: 17.0
            })
        );
        assert_eq!(
            tile.total,
            Text {
                text: "12".into(),
                rect: Rect {
                    x: 17.0,
                    y: 1.0,
                    w: 17.0,
                    h: 15.0
                },
                color: white
            }
        );
        assert_eq!(
            tile.attention,
            Text {
                text: "3".into(),
                rect: Rect {
                    x: 2.0,
                    y: 20.0,
                    w: 32.0,
                    h: 15.0
                },
                color: ON_AMBER
            }
        );
        assert_eq!(tile.font_pixels, 20.0);
        assert_eq!(tile.dot, Some((33.75, 2.25, 2.25)));
    }

    #[test]
    fn a_capped_total_takes_the_whole_row_and_quiet_attention_is_translucent() {
        let black = Rgb::new(0, 0, 0);
        let tile = tile(&Face::counts(120, 0, false), 36.0, black);
        assert_eq!(tile.surface, SURFACE_FOR_DARK_TEXT);
        assert_eq!(tile.mark, None);
        assert_eq!(
            tile.total.rect,
            Rect {
                x: 2.0,
                y: 1.0,
                w: 32.0,
                h: 15.0
            }
        );
        assert_eq!(tile.lower_fill, tile.surface);
        assert_eq!(tile.attention.color, Rgb { a: 160, ..black });
        assert_eq!(tile.dot, None);
    }

    #[test]
    fn the_mark_is_three_blades_from_qt_curves() {
        let steps = mark_path();
        assert_eq!(steps.len(), 30);
        assert_eq!(
            steps
                .iter()
                .filter(|step| matches!(step, Step::Move(..)))
                .count(),
            3
        );
        assert_eq!(
            steps
                .iter()
                .filter(|step| matches!(step, Step::Close))
                .count(),
            3
        );
        let Step::Move(x, y) = steps[0] else {
            panic!("a blade starts with a move")
        };
        assert!(
            (x - 40.5024).abs() < 1e-9 && (y - 9.2112).abs() < 1e-9,
            "({x}, {y})"
        );
    }

    #[test]
    fn pixels_lose_their_premultiplied_alpha() {
        let mut rgba = [128, 64, 0, 128, 10, 10, 10, 0, 10, 20, 30, 255];
        unpremultiply(&mut rgba);
        assert_eq!(rgba, [255, 128, 0, 128, 0, 0, 0, 0, 10, 20, 30, 255]);
    }

    #[test]
    fn icon_colours_read_like_qt() {
        assert_eq!(parse_color("#ff8000"), Some(Rgb::new(255, 128, 0)));
        assert_eq!(parse_color(" #f80 "), Some(Rgb::new(255, 136, 0)));
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(foreground(true, None), Rgb::new(255, 255, 255));
        assert_eq!(foreground(false, None), Rgb::new(0, 0, 0));
        assert_eq!(foreground(true, Some(Rgb::new(1, 2, 3))), Rgb::new(1, 2, 3));
    }
}
