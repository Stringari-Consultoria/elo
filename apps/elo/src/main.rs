#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod settings;
mod ui;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default().with_title("Elo").with_app_id("br.com.stringari.elo").with_inner_size([1100.0, 720.0]).with_min_inner_size([760.0, 520.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/elo-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, viewport, ..Default::default() };
    eframe::run_native("Elo", options, Box::new(|cc| Ok(Box::new(app::EloApp::new(cc)))))
}
