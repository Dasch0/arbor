/// Utilizes wgpu_glyph to render text and provides font and style information
///
use crate::gfx::{self, OUTPUT_FORMAT};
use crate::window;
use wgpu::DepthStencilState;
pub use wgpu_glyph::GlyphBrush;
use wgpu_glyph::{ab_glyph, GlyphBrushBuilder, Section, Text};

/// Enum for all supported fonts, used as an index into the [TextRenderer]'s [glyph_brush]
pub enum Font {
    LoraRegular = 0,
}

/// Table for fonts. Should match the [Font] enum ordering
const FONT_TABLE: &[&'static [u8]] = &[include_bytes!("../data/fonts/Lora-Regular.ttf")];

/// Definitions for style presets. Each preset is an instance of [StyleData]
pub mod styles {
    use super::{Font, Style};

    pub const TITLE: Style = Style {
        font: Font::LoraRegular,
        color: [0.8, 0.8, 0.8, 1.0],
        size: 48.0,
    };
    pub const DIALOGUE: Style = Style {
        font: Font::LoraRegular,
        color: [0.8, 0.8, 0.8, 1.0],
        size: 12.0,
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
    pub fn enqueue(&mut self, style: Style, position: (f32, f32), height: f32, text: &str) {
        // Queue text on top, it will be drawn first.
        // Depth buffer will make it appear on top.
        self.glyph_brush.queue(Section {
            screen_position: position,
            text: vec![Text::default()
                .with_text(text)
                .with_scale(style.size)
                .with_color(style.color)
                .with_z(height)],
            ..Section::default()
        });
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
                &mut context.staging_belt,
                encoder,
                frame.view(),
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
