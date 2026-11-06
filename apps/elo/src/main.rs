#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod i18n_en;

mod app;
mod autostart;
mod empresa;
mod mac;
mod onboarding;
mod settings;
mod tray;
mod ui;
mod update;
mod variant;

fn main() -> eframe::Result {
    let app_id = if variant::client_build() { "br.com.stringari.elo-cliente" } else { "br.com.stringari.elo" };
    let mut viewport = egui::ViewportBuilder::default().with_title(variant::app_name()).with_app_id(app_id).with_inner_size([1100.0, 720.0]).with_min_inner_size([760.0, 520.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/elo-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, viewport, ..Default::default() };
    // Aberto pelo início automático: começa minimizado.
    let background = std::env::args().any(|a| a == autostart::ARG);
    eframe::run_native(variant::app_name(), options, Box::new(move |cc| Ok(Box::new(app::EloApp::new(cc, background)))))
}
