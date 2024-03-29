use crate::{gfx, window};

/// Data for the size and position of a rectangular area. The rectangular area is in screen
/// coordinates. Values are stored as float64 for easy checking against mouse cursor data
///
/// x1y1 -------- x2y1
///   |            |
///   |            |
///   |            |
///   |            |
/// x2y1 -------- x2y2
pub struct Rect {
    pub x1: f64,
    pub x2: f64,
    pub y1: f64,
    pub y2: f64,
}

impl Rect {
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

    /// Create a [Quad] matching the bounds of a [Rect]. Requires knowing the current screen width
    /// and height to accurately create the [Quad]
    pub fn to_quad(&self, context: &gfx::Context, size: window::Size) -> gfx::Quad {
        let center = window::Position::new(size.width as f64 / 2.0, size.height as f64 / 2.0);

        // get normalized coordinates for quad, downcast to f32 for compatibility with GPU format
        let x1_normalized: f32 = ((self.x1 - center.x) / center.x) as f32;
        let x2_normalized: f32 = ((self.x2 - center.x) / center.x) as f32;
        let y1_normalized: f32 = ((self.y1 - center.y) / center.y) as f32;
        let y2_normalized: f32 = ((self.y2 - center.y) / center.y) as f32;

        println!(
            "{}, {}, {}, {}",
            x1_normalized, x2_normalized, y1_normalized, y2_normalized
        );

        gfx::Quad::from_coords(
            context,
            x1_normalized,
            x2_normalized,
            y1_normalized,
            y2_normalized,
        )
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
