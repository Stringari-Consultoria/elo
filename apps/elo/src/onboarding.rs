//! Boas-vindas na primeira abertura: nome do computador, servidor (este
//! computador ou um servidor existente, com teste) e início automático.

use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke};
use elo_core::net::ServerCfg;
use stringari_ui::icons::{self, Icon};
use stringari_ui::theme::{self, Palette};

use crate::app::EloApp;
use crate::settings::Settings;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Este computador roda o servidor.
    Local,
    /// Conecta a um servidor existente.
    Remote,
}

pub struct Onboarding {
    step: usize,
    name: String,
    mode: Mode,
    public: String,
    address: String,
    pin: String,
    autostart: bool,
}

enum Act {
    Test(ServerCfg, bool),
    Finish,
    Skip,
}

impl Onboarding {
    pub fn new(s: &Settings, _autostart: bool) -> Self {
        Self {
            step: 0,
            name: s.device_name.clone(),
            mode: if s.local_server.enabled { Mode::Local } else { Mode::Remote },
            public: s.local_server.public.clone(),
            address: s.server.address.clone(),
            pin: s.server.pin.clone(),
            // Ligado por padrão: o computador continua acessível depois de reiniciar.
            autostart: true,
        }
    }

    fn remote(&self) -> ServerCfg {
        ServerCfg { address: self.address.trim().to_string(), tls: true, pin: self.pin.trim().to_string() }
    }

    fn public_ws(&self) -> Option<String> {
        let p = self.public.trim();
        (p.starts_with("wss://") || p.starts_with("ws://")).then(|| p.to_string())
    }
}

/// Cartão de opção (escolha única).
fn option(ui: &mut egui::Ui, p: &Palette, on: bool, title: &str, desc: &str) -> bool {
    let r = egui::Frame::new()
        .fill(if on { p.surface } else { p.raised })
        .stroke(Stroke::new(if on { 1.5 } else { 1.0 }, if on { p.accent } else { p.border }))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                ui.painter().circle_stroke(rect.center(), 7.0, Stroke::new(1.5, if on { p.accent } else { p.border_strong }));
                if on {
                    ui.painter().circle_filled(rect.center(), 4.0, p.accent);
                }
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).font(theme::semibold(13.5)).color(p.text));
                    ui.add(egui::Label::new(RichText::new(desc).size(12.0).color(p.text2)).wrap());
                });
            });
        })
        .response
        .interact(Sense::click());
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn steps(ui: &mut egui::Ui, p: &Palette, step: usize) {
    ui.horizontal(|ui| {
        for i in 0..3 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(if i == step { 22.0 } else { 8.0 }, 8.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), if i <= step { p.accent } else { p.border });
        }
    });
}

impl EloApp {
    pub fn onboarding_ui(&mut self, ctx: &egui::Context) {
        let Some(mut ob) = self.onboarding.take() else { return };
        self.poll_probe();
        if self.probe.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        let probing = self.probe.is_some();
        let result = self.probe_result.clone();
        let port = self.settings.local_server.port;
        let local_online = self.settings.local_server.enabled && self.my_id().is_some();
        let mut act = None;
        let max_h = (ctx.content_rect().height() - 120.0).max(260.0);
        egui::Modal::new(egui::Id::unique("boas-vindas")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(26))).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.horizontal(|ui| {
                theme::logo(ui, 30.0, Icon::Remote);
                ui.add_space(4.0);
                ui.label(RichText::new("Elo").font(theme::semibold(16.0)).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| steps(ui, &p, ob.step));
            });
            ui.add_space(16.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| match ob.step {
                0 => {
                    ui.label(RichText::new("Bem-vindo ao Elo").font(theme::semibold(20.0)).color(p.text));
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new("Acesso remoto com cifragem ponta a ponta. Em três passos este computador fica pronto para acessar outros e ser acessado.").size(13.0).color(p.text2)).wrap());
                    ui.add_space(16.0);
                    theme::field_label(ui, "Nome deste computador");
                    ui.add(egui::TextEdit::singleline(&mut ob.name).hint_text("ex.: Notebook da recepção").desired_width(f32::INFINITY));
                    ui.label(RichText::new("É o nome que aparece para quem se conecta a ele.").size(11.5).color(p.muted));
                }
                1 => {
                    ui.label(RichText::new("Servidor").font(theme::semibold(20.0)).color(p.text));
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new("O servidor dá os IDs e liga os computadores. Ele não vê o conteúdo das sessões, que é cifrado entre as duas pontas.").size(13.0).color(p.text2)).wrap());
                    ui.add_space(12.0);
                    if option(ui, &p, ob.mode == Mode::Local, "Este computador é o servidor", "O Elo roda o servidor aqui. Bom para o primeiro computador da equipe ou para usar na rede local.") {
                        ob.mode = Mode::Local;
                    }
                    ui.add_space(6.0);
                    if option(ui, &p, ob.mode == Mode::Remote, "Conectar a um servidor existente", "Use o endereço que você recebeu, por exemplo wss://elo.empresa.com.") {
                        ob.mode = Mode::Remote;
                    }
                    ui.add_space(12.0);
                    match ob.mode {
                        Mode::Local => {
                            theme::field_label(ui, "Endereço público (opcional)");
                            ui.horizontal(|ui| {
                                let w = ui.available_width() - 80.0;
                                ui.add(egui::TextEdit::singleline(&mut ob.public).font(theme::mono(12.0)).hint_text("wss://elo.seudominio.com").desired_width(w));
                                let can = ob.public_ws().is_some() && !probing && local_online;
                                if ui.add_enabled(can, egui::Button::new(if probing { "Testando…" } else { "Testar" })).clicked()
                                    && let Some(a) = ob.public_ws()
                                {
                                    act = Some(Act::Test(ServerCfg { address: a, tls: true, pin: String::new() }, true));
                                }
                            });
                            ui.label(RichText::new("Para acessar pela internet sem abrir portas. Na rede local não precisa.").size(11.5).color(p.muted));
                            crate::ui::cloudflare_help(ui, &p, port);
                        }
                        Mode::Remote => {
                            theme::field_label(ui, "Endereço do servidor");
                            ui.horizontal(|ui| {
                                let w = ui.available_width() - 80.0;
                                ui.add(egui::TextEdit::singleline(&mut ob.address).font(theme::mono(12.0)).hint_text("wss://elo.empresa.com ou 192.168.0.10:21116").desired_width(w));
                                let can = !ob.address.trim().is_empty() && !probing;
                                if ui.add_enabled(can, egui::Button::new(if probing { "Testando…" } else { "Testar" })).clicked() {
                                    act = Some(Act::Test(ob.remote(), false));
                                }
                            });
                            if !ob.address.trim().starts_with("ws") {
                                theme::field_label(ui, "Impressão digital do certificado (se o servidor pediu)");
                                ui.add(egui::TextEdit::singleline(&mut ob.pin).font(theme::mono(11.0)).hint_text("vazio para wss:// e certificados públicos").desired_width(f32::INFINITY));
                            }
                        }
                    }
                    if let Some((ok, msg)) = &result {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            icons::icon(ui, if *ok { Icon::Check } else { Icon::Alert }, 13.0, if *ok { p.success } else { p.danger });
                            ui.add(egui::Label::new(RichText::new(msg).size(12.0).color(if *ok { p.success } else { p.danger })).wrap());
                        });
                    }
                }
                _ => {
                    ui.label(RichText::new("Últimos ajustes").font(theme::semibold(20.0)).color(p.text));
                    ui.add_space(12.0);
                    ui.checkbox(&mut ob.autostart, RichText::new("Iniciar o Elo junto com o computador").size(13.5));
                    ui.add(egui::Label::new(RichText::new("Abre minimizado ao entrar no sistema, para este computador continuar acessível depois de reiniciar.").size(12.0).color(p.text2)).wrap());
                    ui.add_space(14.0);
                    ui.add(egui::Label::new(RichText::new("Para acessar este computador sem ninguém do outro lado, defina uma senha fixa em Configurações.").size(12.0).color(p.muted)).wrap());
                }
            });
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                if ob.step == 0 {
                    if ui.add(egui::Label::new(RichText::new("Pular").size(12.5).color(p.muted)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        act = Some(Act::Skip);
                    }
                } else if theme::secondary(ui, None, "Voltar").clicked() {
                    ob.step -= 1;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (text, ok) = match ob.step {
                        0 => ("Começar", !ob.name.trim().is_empty()),
                        1 => ("Continuar", ob.mode == Mode::Local || !ob.address.trim().is_empty()),
                        _ => ("Concluir", true),
                    };
                    if ui.add_enabled_ui(ok, |ui| theme::primary(ui, None, text, None)).inner.clicked() {
                        if ob.step < 2 {
                            ob.step += 1;
                        } else {
                            act = Some(Act::Finish);
                        }
                    }
                });
            });
        });
        match act {
            Some(Act::Test(cfg, mine)) => self.test_server(cfg, mine),
            Some(Act::Skip) => {
                self.settings.onboarded = true;
                self.settings.save();
                return;
            }
            Some(Act::Finish) => {
                self.finish_onboarding(ob);
                return;
            }
            None => {}
        }
        self.onboarding = Some(ob);
    }

    fn finish_onboarding(&mut self, ob: Onboarding) {
        let name = ob.name.trim().to_string();
        let renamed = !name.is_empty() && name != self.settings.device_name;
        if renamed {
            self.settings.device_name = name;
        }
        match ob.mode {
            Mode::Local => {
                self.settings.local_server.enabled = true;
                self.settings.local_server.public = ob.public.trim().to_string();
            }
            Mode::Remote => {
                self.settings.local_server.enabled = false;
                self.settings.server = ob.remote();
            }
        }
        self.settings.onboarded = true;
        self.settings.save();
        if ob.autostart != self.autostart {
            if let Err(e) = crate::autostart::set(ob.autostart) {
                self.error(format!("Iniciar com o computador: {e}"));
            }
            self.autostart = crate::autostart::enabled();
        }
        self.probe_result = None;
        self.reconnect_server();
        if renamed {
            self.restart_control();
        }
        self.toast("Tudo pronto.");
    }
}
