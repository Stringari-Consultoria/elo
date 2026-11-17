#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod i18n_en;

mod app;
mod autologin;
mod autostart;
mod empresa;
mod instance;
mod mac;
mod onboarding;
mod settings;
mod store;
mod tray;
mod ui;
mod update;
mod variant;

fn main() -> eframe::Result {
    // Execução como administrador (pkexec) só para mexer no login do GDM.
    if let Some(code) = autologin::cli() {
        std::process::exit(code);
    }
    let app_id = if variant::client_build() { "br.com.stringari.elo-cliente" } else { "br.com.stringari.elo" };
    let mut viewport = egui::ViewportBuilder::default().with_title(variant::app_name()).with_app_id(app_id).with_inner_size([1100.0, 720.0]).with_min_inner_size([760.0, 520.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/elo-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, viewport, ..Default::default() };
    // Aberto pelo início automático: começa minimizado.
    // (Pela Microsoft Store, a tarefa de inicialização abre sem argumentos.)
    let background = std::env::args().any(|a| a == autostart::ARG) || store::started_by_system();
    // Já tem um Elo aberto com esta configuração: ele mostra a janela e este sai.
    let instance = match instance::claim(&settings::config_dir(), !background) {
        instance::Claim::Running => return Ok(()),
        instance::Claim::First(i) => Some(i),
        instance::Claim::Unknown => None,
    };
    eframe::run_native(variant::app_name(), options, Box::new(move |cc| Ok(Box::new(app::EloApp::new(cc, background, instance)))))
}
