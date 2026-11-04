#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod settings;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("Elo").with_app_id("br.com.stringari.elo").with_inner_size([1100.0, 720.0]).with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("Elo", options, Box::new(|cc| Ok(Box::new(app::EloApp::new(cc)))))
}
