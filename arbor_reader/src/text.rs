/// Utilizes wgpu_glyph to render text and provides font and style information
///
use crate::gfx::{self, Point, OUTPUT_FORMAT};
use crate::ui::Rect;
use crate::window;
use log::warn;
use wgpu::DepthStencilState;
pub use wgpu_glyph::GlyphBrush;
use wgpu_glyph::{ab_glyph, GlyphBrushBuilder, GlyphCruncher, Section, Text};

/// Enum for all supported fonts, used as an index into the [TextRenderer]'s [glyph_brush]
pub enum Font {
    LoraRegular = 0,
}

pub enum Align {
    Left,
    Right,
    Center,
}

/// Table for fonts. Should match the [Font] enum ordering
const FONT_TABLE: &[&[u8]] = &[include_bytes!("../data/fonts/Lora-Regular.ttf")];

/// Definitions for style presets. Each preset is an instance of [StyleData]
pub mod styles {
    use super::{Align, Font, Style};

    pub const TITLE: Style = Style {
        font: Font::LoraRegular,
        color: [0.3, 0.0, 0.0, 1.0],
        size: 128.0,
        align: Align::Center,
    };
    pub const SUBTITLE: Style = Style {
        font: Font::LoraRegular,
        color: [0.8, 0.8, 0.8, 1.0],
        size: 36.0,
        align: Align::Center,
    };
    pub const DIALOGUE: Style = Style {
        font: Font::LoraRegular,
        color: [0.8, 0.8, 0.8, 1.0],
        size: 36.0,
        align: Align::Left,
    };
    pub const METRIC: Style = Style {
        font: Font::LoraRegular,
        color: [0.8, 0.8, 0.8, 1.0],
        size: 24.0,
        align: Align::Left,
    };
    pub const MENU: Style = Style {
        font: Font::LoraRegular,
        color: [0.2, 0.2, 0.2, 1.0],
        size: 64.0,
        align: Align::Center,
    };
    pub const BUTTON: Style = Style {
        font: Font::LoraRegular,
        color: [0.2, 0.2, 0.2, 1.0],
        size: 64.0,
        align: Align::Center,
    };
}

/// StyleData for text types. Contains all information needed by other modules to render text
/// as desired
pub struct Style {
    pub font: Font,
    /// color of the text, stored in RGBA format
    pub color: [f32; 4],
    /// Size of the text. This is an arbitrary/relative value, as the actual drawn size will
    /// depend on the scaling factor during rendering
    pub size: f32,
    /// Alignment of the text when drawn, relative to the screen position point
    pub align: Align,
}

impl Style {
    /// Get the correct line increment amount (in floating point) for the style
    pub fn inc(&self) -> gfx::Point {
        gfx::Point {
            x: 0.0,
            y: self.size as f64,
            z: 0.0,
        }
    }
}

/// Stores data needed to render text
pub struct Renderer {
    /// glyph_brush storing all initialized font data
    glyph_brush: GlyphBrush<DepthStencilState>,
}

impl Renderer {
    /// Initialize resources to render text. Returns a [GlyphBrush] that may be used by [draw_text]
    ///
    /// Scans the provided path and adds all found font files to the GlyphBrush
    ///
    /// # Panics
    /// If the passed slice isn't a valid font binary
    pub fn new(context: &gfx::Context) -> Renderer {
        // Load all the fonts into a vec
        //
        // Implementation note:
        //  Glyph brush implicitly orders fonts by how the order they appear in the vec passed into the
        //  builder. As long as the enum, ordering of the FONT_TABLE, and ordering here are consistent,
        //  the correct font will be selected when using the Fonts enums
        let fonts = FONT_TABLE
            .iter()
            .map(|f| ab_glyph::FontArc::try_from_slice(f).expect("font loading failed"))
            .collect();

        let glyph_brush = GlyphBrushBuilder::using_fonts(fonts)
            .depth_stencil_state(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
            .build(&context.device, OUTPUT_FORMAT);
        Renderer { glyph_brush }
    }

    /// Enqueues text to be drawn by a subsequent call to [draw]
    /// Position is (x, y, z) in winit screen coordinates
    ///
    /// Returns a bounding rect of the text if successfully created
    pub fn enqueue(&mut self, style: Style, position: Point, text: &str) -> Rect {
        // create text section, don't position it yet
        let mut section = Section {
            text: vec![Text::default()
                .with_text(text)
                .with_scale(style.size)
                .with_color(style.color)
                .with_z(position.z as f32)],
            ..Section::default()
        };

        // get the height and width of the text section, handle case where the section is empty
        // (just don't enqueue and return early)
        let rect = match self.glyph_brush.glyph_bounds(section.clone()) {
            Some(val) => val,
            None => {
                warn!("Attempted to enqueue an empty section of text");
                return Rect {
                    x1: 0.0,
                    x2: 0.0,
                    y1: 0.0,
                    y2: 0.0,
                };
            }
        };

        // reposition rect based on style alignment
        let offset_x = match style.align {
            Align::Left => 0.0,
            Align::Center => rect.width() / 2.0,
            Align::Right => rect.width(),
        };
        section.screen_position = (position.x as f32 - offset_x, position.y as f32);

        self.glyph_brush.queue(section.clone());
        self.glyph_brush.glyph_bounds(section).unwrap().into()
    }

    /// Enqueues text to be drawn by a subsequent call to [draw] with predefined bounds
    ///     Style is the style info for the text to display
    ///     Position is a gfx::Point as reference - positioned based on the style information
    ///     Bounds is (x, y) maximum size of the text box to draw
    ///
    pub fn enqueue_with_bounds(
        &mut self,
        style: Style,
        position: Point,
        bounds: (f32, f32),
        text: &str,
    ) -> Rect {
        // create text section, don't position it yet
        let mut section = Section {
            text: vec![Text::default()
                .with_text(text)
                .with_scale(style.size)
                .with_color(style.color)
                .with_z(position.z as f32)],
            bounds,
            ..Section::default()
        };
        // get the height and width of the text section, handle case where the section is empty
        // (just don't enqueue and return early)
        let rect = match self.glyph_brush.glyph_bounds(section.clone()) {
            Some(val) => val,
            None => {
                warn!("Attempted to enqueue an empty section of text");
                return Rect {
                    x1: 0.0,
                    x2: 0.0,
                    y1: 0.0,
                    y2: 0.0,
                };
            }
        };

        // reposition rect based on style alignment
        let offset_x = match style.align {
            Align::Left => 0.0,
            Align::Center => rect.width() / 2.0,
            Align::Right => rect.width(),
        };
        section.screen_position = (position.x as f32 - offset_x, position.y as f32);

        self.glyph_brush.queue(section.clone());
        self.glyph_brush.glyph_bounds(section).unwrap().into()
    }

    /// Draw all text that was queued up
    pub fn draw(
        &mut self,
        context: &mut gfx::Context,
        encoder: &mut gfx::CommandEncoder,
        size: window::Size,
        frame: &gfx::Frame,
    ) {
        // Draw all the text!
        self.glyph_brush
            .draw_queued(
                &context.device,
                encoder,
                &frame.view,
                wgpu::RenderPassDepthStencilAttachment {
                    view: &frame.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(-1.0),
                        store: true,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: true,
                    }),
                },
                size.width,
                size.height,
            )
            .expect("Draw queued");
    }
}

pub mod input {
    use std::str;

    /// List of characters that count as separators when parsing input
    const SEPERATORS: &'static str = " \r\n\t\"\'";

    const BACKSPACE: char = '\u{8}';
    const CTRLZ: char = '\u{1a}';
    const CTRLY: char = '\u{19}';
    const CTRLX: char = '\u{18}';
    const CTRLC: char = '\u{3}';
    const CTRLV: char = '\u{16}';
    const CR: char = '\r';
    const LF: char = '\n';

    #[derive(Debug, Clone, Copy)]
    pub enum Event {
        Insert,
        Remove,
        /// Special case for inserting a separator, used to define undo/redo blocks
        InsertSep,
        /// Special case for inserting a separator, used to define undo/redo blocks
        RemoveSep,
        /// Special case for a boundary starting a section of inserts after a section of removes
        InsertBound,
        /// Special case for a boundary starting a section of removes after a section of inserts
        RemoveBound,
    }

    /// A history of text input events
    #[derive(Debug)]
    pub struct History {
        /// List of events that have occurred
        chars: Vec<char>,
        events: Vec<Event>,
        /// Placement within the list. Cannot be greater than the events list
        pub placement: usize,
    }

    impl History {
        pub fn with_capacity(capacity: usize) -> Self {
            Self {
                chars: Vec::<char>::with_capacity(capacity),
                events: Vec::<Event>::with_capacity(capacity),
                placement: 0,
            }
        }

        /// Undo a single Event. Does nothing if there are no events to undo
        ///
        /// Note: This isn't generally what the user expects, this should be used for more granular
        /// internal undo operations
        pub fn undo_single(&mut self, buf: &mut String) {
            if self.placement < 1 {
                return;
            }

            let index = self.placement - 1;
            match self.events[index] {
                Event::Insert | Event::InsertSep | Event::InsertBound => {
                    buf.pop();
                }
                Event::Remove | Event::RemoveSep | Event::RemoveBound => {
                    buf.push(self.chars[index]);
                }
            }
            self.placement -= 1;
        }

        /// Redo a single Event. Does nothing if we are at the top of the event history
        ///
        /// Note: This isn't generally what the user expects, this should be used for more granular
        /// internal undo operations
        pub fn redo_single(&mut self, buf: &mut String) {
            // cant redo if at the most recent edit
            if self.placement > self.events.len() - 1 {
                return;
            }

            let index = self.placement;
            match self.events[index] {
                Event::Insert | Event::InsertSep | Event::InsertBound => {
                    buf.push(self.chars[index]);
                }
                Event::Remove | Event::RemoveSep | Event::RemoveBound => {
                    buf.pop();
                }
            }

            self.placement += 1;
        }

        /// Undoes a single 'block' of events
        ///
        /// A block is a section of events where a Separator does not appear. This means that if a
        /// section of Insert(char) events is followed by an Insert(space), the entire section will
        /// be undone at once. This maps to what users normally expect from an undo command
        pub fn undo(&mut self, buf: &mut String) {
            // special handler for first character to undo, this way we don't get stuck on
            // separators

            if self.placement < 1 {
                return;
            }
            let index = self.placement - 1;
            match self.events[index] {
                Event::InsertSep => {
                    buf.pop();
                    self.placement -= 1;
                }
                Event::RemoveSep => {
                    buf.push(self.chars[index]);
                    self.placement -= 1;
                }
                Event::InsertBound => {
                    buf.pop();
                    self.placement -= 1;
                    return;
                }
                Event::RemoveBound => {
                    buf.push(self.chars[index]);
                    self.placement -= 1;
                    return;
                }
                Event::Insert => {
                    buf.pop();
                    self.placement -= 1;
                }
                Event::Remove => {
                    buf.push(self.chars[index]);
                    self.placement -= 1;
                }
            }

            // loop over the history, will terminate when a separator is found or the undo history
            // is exhausted
            loop {
                if self.placement < 1 {
                    break;
                }

                let index = self.placement - 1;

                match self.events[index] {
                    Event::InsertSep => {
                        break;
                    }
                    Event::RemoveSep => {
                        break;
                    }
                    Event::InsertBound => {
                        buf.pop();
                        self.placement -= 1;
                        break;
                    }
                    Event::RemoveBound => {
                        buf.push(self.chars[index]);
                        self.placement -= 1;
                        break;
                    }
                    Event::Insert => {
                        buf.pop();
                        self.placement -= 1;
                    }
                    Event::Remove => {
                        buf.push(self.chars[index]);
                        self.placement -= 1;
                    }
                }
            }
        }

        /// Redoes a single 'block' of events
        ///
        /// A block is a section of events where a Separator does not appear. This means that if a
        /// section of Insert(char) events is followed by an Insert(space), the entire section will
        /// be undone at once. This maps to what users normally expect from an undo command
        pub fn redo(&mut self, buf: &mut String) {
            // Special case to handle the first redo, prevents getting stuck on boundaries
            if self.placement > self.events.len() - 1 {
                return;
            }

            let index = self.placement;

            match self.events[index] {
                Event::InsertSep => {
                    buf.push(self.chars[index]);
                    self.placement += 1;
                }
                Event::RemoveSep => {
                    buf.pop();
                    self.placement += 1;
                }
                Event::InsertBound => {
                    buf.push(self.chars[index]);
                    self.placement += 1;
                    return;
                }
                Event::RemoveBound => {
                    buf.pop();
                    self.placement += 1;
                    return;
                }
                Event::Insert => {
                    buf.push(self.chars[index]);
                    self.placement += 1;
                }
                Event::Remove => {
                    buf.pop();
                    self.placement += 1;
                }
            };
            // loop over the history, will terminate when a separator is found or the redo reaches
            // the top of the history.
            loop {
                if self.placement > self.events.len() - 1 {
                    break;
                }

                let index = self.placement;

                match self.events[index] {
                    Event::InsertSep => {
                        break;
                    }
                    Event::RemoveSep => {
                        break;
                    }
                    Event::InsertBound => {
                        break;
                    }
                    Event::RemoveBound => {
                        break;
                    }
                    Event::Insert => {
                        buf.push(self.chars[index]);
                        self.placement += 1;
                    }
                    Event::Remove => {
                        buf.pop();
                        self.placement += 1;
                    }
                };
            }
        }

        /// Push a new event onto the history. This while wipe away all 'redos' beyond the current
        /// placement within the events list. This matches the expected behavior of other undo/redo
        /// applications
        pub fn push(&mut self, event: Event, c: char) {
            // clear the events list past the current placement
            self.events.truncate(self.placement);
            self.chars.truncate(self.placement);
            self.events.push(event);
            self.chars.push(c);
            // set placement to end of the events
            self.placement = self.events.len();
        }
    }

    /// parse text input from any source that can provide an iterator over the input
    /// chars. Text is appended to the supplied buffer
    pub fn parse(buf: &mut String, input: str::Chars, history: &mut History) {
        input.for_each(|c| {
            // check previous event, used to determine if undo unit separator needs to be added
            let prev_event = match history.placement {
                0 => Event::InsertSep,
                _ => history.events[history.placement - 1],
            };

            // match character against different cases, update the buffer and history accordingly
            match c {
                BACKSPACE => {
                    if let Some(b) = buf.pop() {
                        // determine if at an undo unit separator based on previous events
                        let mut event_type = match prev_event {
                            Event::Insert | Event::InsertSep | Event::InsertBound => {
                                Event::RemoveBound
                            }
                            Event::Remove | Event::RemoveSep | Event::RemoveBound => Event::Remove,
                        };
                        // override event_type if the actual character is a separator
                        if SEPERATORS.contains(b) {
                            event_type = Event::RemoveSep;
                        };
                        history.push(event_type, b);
                    }
                }
                CTRLZ => history.undo(buf),
                CTRLY => history.redo(buf),
                _ => {
                    buf.push(c);
                    let mut event_type = match prev_event {
                        Event::Insert | Event::InsertSep | Event::InsertBound => Event::Insert,
                        Event::Remove | Event::RemoveSep | Event::RemoveBound => Event::InsertBound,
                    };
                    // override event_type if the actual character is a separator
                    if SEPERATORS.contains(c) {
                        event_type = Event::InsertSep;
                    };
                    history.push(event_type, c);
                }
            }
        });
    }

    /// parse single line of input (no newline) from any source. Undo/redo is not supported for these lines
    pub fn parse_single_no_history(buf: &mut String, input: str::Chars) {
        input.for_each(|c| {
            // match character against different cases, update the buffer and history accordingly
            match c {
                BACKSPACE => {
                    buf.pop();
                }
                CTRLZ => {}
                CTRLY => {}
                CR => {}
                LF => {}
                _ => buf.push(c),
            };
        });
    }
}
