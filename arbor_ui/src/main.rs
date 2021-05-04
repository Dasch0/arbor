mod ui;
mod util;

// When compiling natively:
fn main() {
    let app = ui::ArborUi::default();
    eframe::run_native(Box::new(app));
}
