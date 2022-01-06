use crate::{gfx, window};
use glyph_brush::ab_glyph;

/// Screen Coordinates:
/// x1y1 -------- x2y1
///   |            |
///   |            |
///   |            |
///   |            |
/// x2y1 -------- x2y2

/// Data for the size and position of a rectangular area. The rectangular area is in screen
/// coordinates. Values are stored as float64 for easy checking against mouse cursor data
///
#[derive(Debug)]
pub struct Rect {
    pub x1: f64,
    pub x2: f64,
    pub y1: f64,
    pub y2: f64,
}

impl Rect {
    /// Get a gfx::Point corresponding to the center of the rect at z = 0.0
    pub fn pt(&self) -> gfx::Point {
        gfx::Point {
            x: (self.x1 + self.x2) / 2.0,
            y: (self.y1 + self.y2) / 2.0,
            z: 0.0,
        }
    }

    /// Create a new Rect from a tuple of the (top left) x-y origin, and the desired width/height
    pub fn from_tuple((x, y, width, height): (f64, f64, f64, f64)) -> Self {
        Self {
            x1: x,
            x2: x + width,
            y1: y,
            y2: y + height,
        }
    }

    /// Create a new rect from raw coordinates
    pub fn from_coords(x1: f64, x2: f64, y1: f64, y2: f64) -> Self {
        Self { x1, x2, y1, y2 }
    }

    /// Convenience method to check if the cursor is hovering over the rect this frame
    #[inline]
    pub fn hovered(&self, input: &window::Input) -> bool {
        (self.x1 <= input.cursor_position.x)
            && (self.x2 >= input.cursor_position.x)
            && (self.y1 <= input.cursor_position.y)
            && (self.y2 >= input.cursor_position.y)
    }

    /// Convenience method to check if the rectangle has been clicked this frame
    #[inline]
    pub fn clicked(&self, input: &window::Input) -> bool {
        input.cursor_pressed() && self.hovered(input)
    }

    /// Convenience method to check if the cursor is held down over the rect this frame
    #[inline]
    pub fn held(&self, input: &window::Input) -> bool {
        input.cursor_held() && self.hovered(input)
    }

    /// Convenience method to check if the cursor just let go of the rect this frame
    #[inline]
    pub fn released(&self, input: &window::Input) -> bool {
        input.cursor_released() && self.hovered(input)
    }
}

impl From<ab_glyph::Rect> for Rect {
    fn from(item: ab_glyph::Rect) -> Rect {
        Rect {
            x1: item.min.x as f64,
            y1: item.min.y as f64,
            x2: item.max.x as f64,
            y2: item.max.y as f64,
        }
    }
}
