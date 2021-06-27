/// Encapsulates the winit callbacks and control flow and instead presents a simple state struct
/// that can be queried per frame.
///
/// Adapted from crate winit_input_helper @ https://crates.io/crates/winit_input_helper/0.10.0/dependencies
pub use winit::dpi::PhysicalSize;
pub use winit::event::{Event, VirtualKeyCode, WindowEvent};

/// Stores state of window actions, created from a raw winit handle
pub struct WindowState {
    input: Option<u32>,
    window_resized: Option<PhysicalSize<u32>>,
    window_size: PhysicalSize<u32>,
    scale_factor_changed: Option<f64>,
    pub scale_factor: f64,
    pub draw: bool,
    pub quit: bool,
}

impl WindowState {
    pub fn new(window: &winit::window::Window) -> Self {
        Self {
            input: None,
            window_resized: None,
            window_size: window.inner_size(),
            scale_factor_changed: None,
            scale_factor: window.scale_factor(),
            draw: true,
            quit: false,
        }
    }

    /// Updates the windowState based on the winit events occuring this frame
    pub fn update<T>(&mut self, event: &Event<T>) -> bool {
        match event {
            Event::NewEvents(_) => {
                self.prepare();
                false
            }
            Event::WindowEvent { event, .. } => {
                self.process_window_event(event);
                false
            }
            Event::MainEventsCleared => true,
            _ => false,
        }
    }

    /// Prepare window state to handle new events
    fn prepare(&mut self) {
        self.window_resized = None;
        self.scale_factor_changed = None;
        self.input = None;
    }

    fn process_window_event(&mut self, event: &WindowEvent) {}
}
