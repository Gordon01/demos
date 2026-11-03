mod analysis;
mod app;
mod audio;
mod system_audio;
mod visualization;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_title("Frequency Visualiser"),
        ..Default::default()
    };
    eframe::run_native(
        "Frequency Visualiser",
        options,
        Box::new(|_creation_context| Ok(Box::new(app::VisualiserApp::new()))),
    )
}
