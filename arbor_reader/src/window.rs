/// Encapsulates the winit callbacks and control flow and instead presents a simple state struct
/// that can be queried per frame.
///
/// Adapted from crate winit_input_helper @ https://crates.io/crates/winit_input_helper/0.10.0/dependencies
///
/// All inner match statements where possible adhere to conditional moves to avoid excess branching
///
pub use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Event, MouseButton, TouchPhase, WindowEvent};

/// Stores state of window actions, created from a raw winit handle
pub struct WindowState {
    /// Tracked state for user input
    pub input: Input,
    /// Size of the window
    pub size: PhysicalSize<u32>,
    /// DPI scaling currently in place in the window
    pub scale: f64,
    /// status flag indicating the app should resize
    pub resize: bool,
    /// status flag indicating the app should respond to new DPI scaling
    pub rescale: bool,
    /// status flag indicating the app should quit
    pub quit: bool,
}

impl WindowState {
    pub fn new(window: &winit::window::Window) -> Self {
        Self {
            input: Input::new(),
            size: window.inner_size(),
            scale: window.scale_factor(),
            resize: false,
            rescale: false,
            quit: false,
        }
    }

    /// Updates the windowState based on the winit events occuring this frame
    ///
    /// Update should be repeatedly called until it returns false to collect all events
    pub fn update<T>(&mut self, event: Event<T>) -> bool {
        match event {
            Event::NewEvents(_) => {
                self.prepare();
                true
            }
            Event::WindowEvent { event, .. } => {
                self.process_window_event(event);
                true
            }
            // when all main events are cleared, update loop is done
            Event::MainEventsCleared => false,

            // Redraw requested should immediately end the update loop to allow a damaged frame to
            // be redrawn. Per winit docs RedrawRequest *should* happen after all input events.
            // Source: https://github.com/rust-windowing/winit/issues/1619
            Event::RedrawRequested(..) => false,
            _ => true,
        }
    }

    /// Prepare window state to handle new events
    fn prepare(&mut self) {
        self.resize = false;
        self.rescale = false;
        self.input.prepare();
    }

    fn process_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.quit = true,
            WindowEvent::Destroyed => self.quit = true,
            WindowEvent::Resized(size) => {
                self.resize = true;
                self.size = size;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.rescale = true;
                self.scale = scale_factor;
            }
            _ => self.input.process_input_event(event),
        }
    }
}

/// Implement this trait to handle events and interact with the window
pub struct Input {
    cursor_position: PhysicalPosition<f64>,
    pub text: String,
    cursor_pressed: bool,
    cursor_last_pressed: bool,
}

impl Input {
    /// Create a new input container
    fn new() -> Self {
        Self {
            cursor_position: PhysicalPosition::<f64>::new(0.0, 0.0),
            text: String::with_capacity(100),
            cursor_pressed: false,
            cursor_last_pressed: false,
        }
    }

    /// Prepare to accept new inputs, called at the beginning of each frame
    fn prepare(&mut self) {
        self.cursor_last_pressed = self.cursor_pressed;
        self.text.clear();
    }

    fn process_input_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::ReceivedCharacter(c) => self.text.push(c),
            WindowEvent::CursorMoved { position, .. } => self.cursor_position = position,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.cursor_pressed = true,
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.cursor_pressed = false,
            WindowEvent::Touch(touch) => {
                self.cursor_position = touch.location;
                match touch.phase {
                    TouchPhase::Started => self.cursor_pressed = true,
                    TouchPhase::Moved => self.cursor_pressed = true,
                    TouchPhase::Ended => self.cursor_pressed = false,
                    TouchPhase::Cancelled => self.cursor_pressed = false,
                }
            }
            _ => {}
        }
    }

    /// Check if the cursor was just pressed
    pub fn cursor_pressed(&self) -> bool {
        self.cursor_pressed & !self.cursor_last_pressed
    }

    /// Check if the cursor was just released
    pub fn cursor_released(&self) -> bool {
        !self.cursor_pressed & self.cursor_last_pressed
    }

    /// Check if the cursor is being held
    pub fn cursor_held(&self) -> bool {
        self.cursor_pressed & self.cursor_last_pressed
    }
}

/// Convenience function to create a winit window and WindowState handle
pub fn init(
    title: &'static str,
    width: u32,
    height: u32,
) -> (
    winit::event_loop::EventLoop<()>,
    winit::window::Window,
    WindowState,
) {
    let event_loop = winit::event_loop::EventLoop::with_user_event();
    let window_handle = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title(title)
        .with_inner_size(winit::dpi::PhysicalSize { width, height })
        .build(&event_loop)
        .unwrap();

    let window_state = WindowState::new(&window_handle);

    (event_loop, window_handle, window_state)
}
