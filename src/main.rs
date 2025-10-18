use eframe::egui::{Button, Color32, Pos2, Rect, Stroke};
use eframe::{CreationContext, egui};

mod bt;
mod data;
mod ui;

fn main() {
    // Run the GUI in the main thread.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 480.0])
            .with_fullscreen(true),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|cc| Ok(Box::<ui::MyApp>::new(ui::MyApp::new(cc)))),
    )
    .expect("Failed to start gui");
}
