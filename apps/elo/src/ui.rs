//! Interface do Elo.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Vec2};
use std::sync::{Arc, Mutex};

use elo_core::session::{Cmd, Remote};
use elo_proto::ctrl::pretty_id;
use elo_proto::msg::{Button, Input, Key};
use stringari_ui::icons::{self, Icon};
use stringari_ui::theme::{self, Palette, pal};

use crate::app::{ChatLine, EloApp, RemoteInput, SplitWin, Status, ViewSession, quality};
use crate::mac::{self, Kind};

pub(crate) fn card(p: &Palette) -> egui::Frame {
    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(20))
}

pub(crate) fn copy(ui: &egui::Ui, t: &str) {
    ui.ctx().copy_text(t.to_string());
}

fn human_bytes(b: u64) -> String {
    let b = b as f64;
    if b > 1e9 {
        t!("{} GB", format!("{:.1}", b / 1e9))
    } else if b > 1e6 {
        t!("{} MB", format!("{:.1}", b / 1e6)
    )
    } else {
        t!("{} KB", format!("{:.0}", b / 1e3))
    }
    .replace('.', ",")
}

impl eframe::App for EloApp {
    /// Roda mesmo com a janela escondida ou minimizada: rede, convites,
    /// sessões, atualizações e bandeja não podem parar.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        self.tray_tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.start_minimized) {
            // Com bandeja, some de vez; sem, fica minimizado na barra de tarefas.
            if self.tray.is_some() {
                self.hidden = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        // Fechar a janela: vai para a bandeja (a não ser que tenha pedido para sair).
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.settings.close_to_tray && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
            self.set_fullscreen(&ctx, false);
        }
        let p = pal(ui);
        // Tela cheia só faz sentido numa sessão aberta.
        let fs = self.fullscreen && self.active_tab > 0 && self.active_tab <= self.views.len();
        if self.fullscreen && !fs {
            self.set_fullscreen(&ctx, false);
        }
        // Barra de título.
        if !fs {
            egui::Panel::top("top").exact_size(44.0).frame(egui::Frame::new().fill(p.chrome).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin { left: 14, right: 12, ..Default::default() })).show(ui, |ui| self.top_bar(ui));
        }
        // Faixa "sendo acessado".
        if self.hosts.iter().any(|h| h.started) {
            let in_session = self.active_tab > 0 && self.active_tab <= self.views.len();
            let under = if in_session { Color32::from_gray(24) } else { p.bg };
            egui::Panel::top("hosting").frame(egui::Frame::new().fill(under).inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 2 })).show(ui, |ui| self.host_banner(ui));
        }
        if !fs {
            egui::Panel::bottom("status").exact_size(26.0).frame(egui::Frame::new().fill(p.chrome).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin::symmetric(12, 0))).show(ui, |ui| self.status_bar(ui));
        }
        let tab = self.active_tab;
        // Sessão fora da aba aberta não recebe eventos: não pode ficar nada apertado.
        for (i, v) in self.views.iter_mut().enumerate() {
            if i + 1 != tab {
                release_remote_input(v, true);
            }
        }
        if tab == 0 || tab > self.views.len() {
            self.active_tab = 0;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::same(24))).show(ui, |ui| {
                if self.client_mode() {
                    self.client_home(ui);
                } else if self.clients_open && self.overview.is_some() {
                    self.clients_page(ui);
                } else {
                    self.home(ui);
                }
            });
        } else {
            let idx = tab - 1;
            if self.views[idx].files.open {
                let downloads = self.settings.downloads.clone();
                egui::Panel::right("arquivos").default_size(460.0).min_size(360.0).frame(session_panel_frame()).show(ui, |ui| {
                    ui.style_mut().visuals = egui::Visuals::dark();
                    files_panel(ui, &mac::DARK, &mut self.views[idx], &downloads);
                });
            } else if self.views[idx].chat_open {
                egui::Panel::right("chat").default_size(320.0).min_size(260.0).frame(session_panel_frame()).show(ui, |ui| {
                    ui.style_mut().visuals = egui::Visuals::dark();
                    let m = mac::DARK;
                    let v = &mut self.views[idx];
                    let who = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(t!("Mensagens")).font(theme::semibold(14.0)).color(m.label));
                            ui.label(RichText::new(t!("com {}", who)).size(11.5).color(m.label2));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if mac::round_icon(ui, &m, Icon::Close, t!("Fechar"), 26.0, true).clicked() {
                                v.chat_open = false;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    let cmd = v.handle.cmd.clone();
                    chat_panel(ui, &m, &mut v.chat, &mut v.chat_input, &v.transfers, |t| {
                        let _ = cmd.send(Cmd::Chat(t));
                    });
                });
            }
            let central = egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(24))).show(ui, |ui| self.remote_view(ui, idx)).response.rect;
            self.drop_files(&ctx, idx, central);
            match self.session_act.take() {
                Some(SessAct::BarSide) => {
                    self.settings.bar_right = !self.settings.bar_right;
                    // Só esta escolha vai para o disco (o resto fica como estava lá).
                    let mut disk = crate::settings::Settings::load();
                    disk.bar_right = self.settings.bar_right;
                    disk.save();
                }
                Some(SessAct::Shot(area)) => self.take_shot(idx, area),
                Some(SessAct::Fullscreen) => {
                    let on = !self.fullscreen;
                    self.set_fullscreen(&ctx, on);
                }
                Some(SessAct::Favorite) => {
                    let v = &self.views[idx];
                    let (id, name, os) = (v.target_id.clone(), v.target_name.clone(), v.os.clone());
                    // A senha que abriu esta sessão vai junto (vazia se entrou pedindo permissão).
                    let pw = if v.connected { v.password.clone() } else { String::new() };
                    let on = self.settings.toggle_favorite(&id, &name, &os, &pw);
                    self.settings.save();
                    self.toast(match (on, pw.is_empty()) {
                        (true, false) => "Salvo nos favoritos com a senha. Na próxima vez conecta direto; dá para renomear na tela inicial.",
                        (true, true) => "Salvo nos favoritos. Na tela inicial dá para renomear e guardar a senha do acesso não assistido.",
                        _ => "Removido dos favoritos.",
                    });
                }
                None => {}
            }
        }
        self.split_windows(&ctx);
        self.dialogs(&ctx);
        self.onboarding_ui(&ctx);
        self.toasts_ui(&ctx);
        // Sessões encerradas: avisa e fecha a aba.
        let mut closed = Vec::new();
        for (i, v) in self.views.iter().enumerate() {
            if let Some(r) = &v.ended {
                let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
                if r == elo_proto::msg::BYE_RESTART {
                    // Reconecta quando voltar (o Elo de lá precisa iniciar com o computador).
                    let now = std::time::Instant::now();
                    self.reconnects.retain(|x| x.id != v.target_id);
                    self.reconnects.push(crate::app::Reconnect { id: v.target_id.clone(), name: name.clone(), password: v.password.clone(), next_try: now + std::time::Duration::from_secs(20), until: now + std::time::Duration::from_secs(360) });
                    closed.push((i, t!("{} está reiniciando. O Elo reconecta sozinho quando ele voltar.", name)));
                } else if r.starts_with("senha incorreta") && v.password == self.settings.saved_password(&v.target_id).unwrap_or_default() {
                    closed.push((i, t!("{}: a senha salva no favorito não foi aceita. Se a senha fixa de lá mudou, atualize no lápis do cartão do favorito.", name)));
                } else {
                    closed.push((i, t!("{}: {}", name, r)));
                }
            }
        }
        for (i, msg) in closed.into_iter().rev() {
            self.views.remove(i);
            if self.active_tab > i {
                self.active_tab = self.active_tab.saturating_sub(1).max(if self.views.is_empty() { 0 } else { 1 }.min(self.active_tab));
            }
            self.toast(msg);
        }
        if self.active_tab > self.views.len() {
            self.active_tab = self.views.len();
        }
    }

    fn on_exit(&mut self) {
        for v in &self.views {
            v.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        for h in &self.hosts {
            h.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = self.control.tx.send(elo_core::client::Command::Quit);
        self.settings.save();
        // Para o servidor local (derruba as conexões dos outros computadores).
        self.local.server = None;
    }
}

impl EloApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.horizontal_centered(|ui| {
            theme::logo(ui, 20.0, Icon::Remote);
            ui.label(RichText::new(crate::variant::app_name()).font(theme::semibold(14.0)).color(p.text));
            ui.add_space(2.0);
            by_stringari(ui, &p);
            ui.add_space(14.0);
            // Abas: início + sessões.
            let tab = |ui: &mut egui::Ui, on: bool, text: &str| -> egui::Response {
                let fill = if on { p.surface } else { Color32::TRANSPARENT };
                egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, if on { p.border_strong } else { Color32::TRANSPARENT }))
                    .corner_radius(CornerRadius::same(7))
                    .inner_margin(Margin::symmetric(12, 5))
                    .show(ui, |ui| { ui.label(RichText::new(text).size(12.5).color(if on { p.text } else { p.text2 }))})
                    .response
                    .interact(Sense::click())
            };
            if tab(ui, self.active_tab == 0 && !self.clients_open, t!("Início")).clicked() {
                self.active_tab = 0;
                self.clients_open = false;
            }
            // Técnico logado: empresas, computadores e pedidos de ajuda.
            if let Some(o) = &self.overview {
                let waiting = o.requests.iter().filter(|r| r.taken_by.is_none()).count();
                let text = if waiting > 0 { t!("Clientes ({})", waiting) } else { t!("Clientes").to_string() };
                let r = tab(ui, self.active_tab == 0 && self.clients_open, &text);
                if waiting > 0 {
                    ui.painter().circle_filled(r.rect.right_top() + Vec2::new(-3.0, 3.0), 4.0, p.danger);
                }
                if r.on_hover_text(t!("Empresas atendidas, computadores e pedidos de ajuda")).clicked() {
                    self.active_tab = 0;
                    self.clients_open = true;
                }
            }
            let mut close = None;
            for (i, v) in self.views.iter().enumerate() {
                let name = match self.settings.favorite(&v.target_id) {
                    Some(f) => f.label(),
                    None if v.target_name.is_empty() => pretty_id(&v.target_id),
                    None => v.target_name.clone(),
                };
                let r = tab(ui, self.active_tab == i + 1, &t!("{}  ×", name));
                if r.clicked() {
                    // Clique no "×" (fim da aba) encerra.
                    if ui.input(|i| i.pointer.interact_pos()).is_some_and(|pos| pos.x > r.rect.right() - 22.0) {
                        close = Some(i);
                    } else {
                        self.active_tab = i + 1;
                    }
                }
            }
            if let Some(i) = close {
                self.views[i].handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Gear, t!("Configurações"), false).clicked() {
                    self.show_settings = true;
                }
                let icon = if self.settings.dark { Icon::Sun } else { Icon::Moon };
                if theme::icon_button(ui, icon, t!("Tema claro/escuro"), false).clicked() {
                    self.settings.dark = !self.settings.dark;
                    theme::apply(ui.ctx(), self.settings.dark);
                    self.settings.save();
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.horizontal_centered(|ui| {
            let (c, t) = match &self.status {
                Status::Online(_) => (p.success, t!("Pronto para conexões").to_string()),
                Status::Connecting => (p.warning, t!("Conectando ao servidor…").to_string()),
                Status::Offline(r) => (p.danger, t!("Sem conexão: {}", r)),
            };
            theme::dot(ui, c, 8.0);
            ui.label(RichText::new(t).size(11.5).color(p.text2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.settings.local_server.enabled {
                    let (t, c) = match &self.local.error {
                        Some(_) => (t!("servidor local com erro").to_string(), p.danger),
                        None => (t!("servidor local · {}", self.local.started.public_ws().map(str::to_string).unwrap_or_else(|| if self.local.started.lan { format!("{} · TLS", self.local.lan_address()) } else { "só este computador".into() })), p.muted),
                    };
                    ui.label(RichText::new(t).font(theme::mono(11.0)).color(c)).on_hover_text(self.local.error.clone().unwrap_or_else(|| t!("Servidor rodando neste computador. Outros computadores conectam neste endereço (Configurações → Servidor).").into()));
                } else {
                    let a = match self.settings.server.address.trim() {
                        "" => "sem servidor configurado",
                        a => a,
                    };
                    let tls = if self.settings.server.address.trim().is_empty() || a.starts_with("wss://") { "" } else if a.starts_with("ws://") || !self.settings.server.tls { t!(" · sem TLS") } else { t!(" · TLS") };
                    ui.label(RichText::new(format!("{a}{tls}")).font(theme::mono(11.0)).color(p.muted));
                }
                icons::icon(ui, Icon::Lock, 11.0, p.muted).on_hover_text(t!("Sessões cifradas ponta a ponta (SPAKE2 + ChaCha20-Poly1305). O servidor não tem a chave."));
            });
        });
    }

    /// Aviso de que este computador está sendo acessado: quem, há quanto
    /// tempo, o que pode fazer (clique liga/desliga), chat e Encerrar.
    fn host_banner(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let m = mac::of(self.settings.dark);
        let mut stop = None;
        let mut perms_change = Vec::new();
        let compact = ui.available_width() < 980.0;
        let now = ui.input(|i| i.time);
        // O ponto "ao vivo" pulsa devagar.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        for (i, h) in self.hosts.iter_mut().enumerate().filter(|(_, h)| h.started) {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0, theme::alpha(m.orange, 0.5)))
                .corner_radius(CornerRadius::same(14))
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if m.dark { 90 } else { 22 }) })
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        // Ícone com o ponto "ao vivo".
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                        mac::icon_tile(ui.painter(), r, Icon::Eye, m.orange);
                        let dot = r.right_top() + Vec2::new(-2.0, 2.0);
                        let t = ((now % 1.6) / 1.6) as f32;
                        ui.painter().circle_stroke(dot, 5.0 + 7.0 * t, Stroke::new(2.0, theme::alpha(m.red, 0.6 * (1.0 - t))));
                        ui.painter().circle_filled(dot, 5.5, p.surface);
                        ui.painter().circle_filled(dot, 4.0, m.red);
                        ui.add_space(6.0);
                        // Quem e há quanto tempo.
                        let right_w = if compact { 286.0 } else { 540.0 };
                        let text_w = (ui.available_width() - right_w).max(120.0);
                        ui.allocate_ui_with_layout(Vec2::new(text_w, 38.0), Layout::top_down(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.add(egui::Label::new(RichText::new(t!("{} está acessando este computador", h.from_name)).font(theme::semibold(13.5)).color(p.text)).truncate());
                            let mins = h.since.elapsed().as_secs() / 60;
                            let how = if h.with_password { t!("entrou com a senha") } else { t!("você aceitou") };
                            let when = if mins == 0 { "agora".to_string() } else { t!("{} min", mins) };
                            ui.add(egui::Label::new(RichText::new(t!("ID {} · código {} · {} · {}", pretty_id(&h.from_id), h.sas, how, when)).size(11.5).color(p.text2)).truncate())
                                .on_hover_text(t!("Peça para a outra pessoa confirmar o código: se for igual nos dois lados, ninguém está no meio."));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if mac::button(ui, &m, t!("Encerrar"), Kind::Destructive, None, true).on_hover_text(t!("Desconecta quem está acessando")).clicked() {
                                stop = Some(i);
                            }
                            ui.add_space(6.0);
                            let chat_tip = if h.chat.is_empty() { t!("Chat").to_string() } else { t!("Chat ({} mensagens)", h.chat.len()) };
                            let c = mac::round_icon(ui, &m, Icon::Comment, &chat_tip, 30.0, true);
                            if h.chat_open {
                                ui.painter().circle_stroke(c.rect.center(), 15.0, Stroke::new(1.5, m.accent));
                            }
                            if !h.chat.is_empty() && !h.chat_open {
                                let b = c.rect.right_top() + Vec2::new(-3.0, 3.0);
                                ui.painter().circle_filled(b, 7.0, m.red);
                                ui.painter().text(b, Align2::CENTER_CENTER, h.chat.len().min(9).to_string(), theme::semibold(9.5), Color32::WHITE);
                            }
                            if c.clicked() {
                                h.chat_open = !h.chat_open;
                            }
                            if mac::round_icon(ui, &m, Icon::Upload, t!("Enviar um arquivo para quem está acessando"), 30.0, true).clicked()
                                && let Some(f) = rfd::FileDialog::new().pick_file()
                            {
                                let _ = h.handle.cmd.send(Cmd::SendFile(f));
                            }
                            ui.add_space(4.0);
                            let (d, _) = ui.allocate_exact_size(Vec2::new(1.0, 22.0), Sense::hover());
                            ui.painter().rect_filled(d, CornerRadius::ZERO, m.sep);
                            ui.add_space(4.0);
                            // O que a pessoa pode fazer: clique liga/desliga.
                            if perm_chip(ui, &m, Icon::Speaker, t!("Som"), h.audio, compact).clicked() {
                                h.audio = !h.audio;
                                let _ = h.handle.cmd.send(Cmd::Audio(h.audio));
                            }
                            let mut pm = h.permissions;
                            for (icon, label, on) in [(Icon::Folder, t!("Arquivos"), &mut pm.files), (Icon::Copy, t!("Copiar/colar"), &mut pm.clipboard), (Icon::Cursor, t!("Controle"), &mut pm.control)] {
                                if perm_chip(ui, &m, icon, label, *on, compact).clicked() {
                                    *on = !*on;
                                }
                            }
                            if pm != h.permissions {
                                h.permissions = pm;
                                perms_change.push((i, pm));
                            }
                        });
                    });
                    // Chat aberto: dentro do cartão.
                    if h.chat_open {
                        ui.add_space(8.0);
                        let (sep, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                        ui.painter().rect_filled(sep, CornerRadius::ZERO, m.sep);
                        ui.add_space(8.0);
                        let cmd = h.handle.cmd.clone();
                        ui.allocate_ui(Vec2::new(ui.available_width(), 230.0), |ui| {
                            ui.set_max_height(230.0);
                            chat_panel(ui, &m, &mut h.chat, &mut h.chat_input, &h.transfers, |t| {
                                let _ = cmd.send(Cmd::Chat(t));
                            });
                        });
                    }
                });
            ui.add_space(8.0);
        }
        for (i, pm) in perms_change {
            let _ = self.hosts[i].handle.cmd.send(Cmd::Permissions(pm));
        }
        if let Some(i) = stop {
            self.hosts[i].handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |outer| {
            // Coluna central de até 1040 px (com folga para a barra de rolagem).
            let full = outer.available_rect_before_wrap();
            let w = (full.width() - 28.0).min(1040.0);
            let x0 = full.center().x - w / 2.0;
            let mut inner = outer.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::pos2(x0, full.top()), Vec2::new(w, f32::INFINITY))).layout(Layout::top_down(Align::Min)));
            let ui = &mut inner;
            {
                if self.update_banner(ui, &p) {
                    self.update_and_restart();
                }
                let mut cancel = None;
                for r in &self.reconnects {
                    egui::Frame::new().fill(theme::alpha(p.warning, 0.12)).stroke(Stroke::new(1.0, theme::alpha(p.warning, 0.4))).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spinner();
                            let left = r.until.saturating_duration_since(std::time::Instant::now()).as_secs();
                            ui.label(RichText::new(t!("{} está reiniciando. Reconectando quando voltar… ({}:{})", r.name, left / 60, format!("{:02}", left % 60))).size(13.0).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::secondary(ui, None, t!("Cancelar")).clicked() {
                                    cancel = Some(r.id.clone());
                                }
                            });
                        });
                    });
                    ui.add_space(10.0);
                }
                if let Some(id) = cancel {
                    self.reconnects.retain(|r| r.id != id);
                }
                ui.add_space(4.0);
                // Cabeçalho.
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t!("Acesso remoto")).font(theme::semibold(22.0)).color(p.text));
                        ui.label(RichText::new(t!("Passe seu ID para receber ajuda, ou digite o ID de outro computador para acessá-lo.")).size(13.0).color(p.text2));
                    });
                });
                ui.add_space(18.0);
                let gap = 16.0;
                let col = (w - gap) / 2.0;
                let top = ui.cursor().min.y;
                let left_x = x0;
                // Coluna da esquerda: destaque com o ID + acesso não assistido.
                let mut left = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::pos2(left_x, top), Vec2::new(col, 1000.0))).layout(Layout::top_down(Align::Min)));
                self.hero(&mut left, &p);
                left.add_space(12.0);
                self.unattended_card(&mut left, &p);
                let left_bottom = left.min_rect().bottom();
                // Coluna da direita: acessar outro computador.
                let mut right = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::pos2(left_x + col + gap, top), Vec2::new(col, 1000.0))).layout(Layout::top_down(Align::Min)));
                self.connect_card(&mut right, &p, left_bottom - top);
                let bottom = left_bottom.max(right.min_rect().bottom());
                ui.allocate_space(Vec2::new(w, bottom - top));
                // Computadores salvos e recentes.
                ui.add_space(26.0);
                self.computers(ui, &p);
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    let text = t!("O conteúdo das sessões é cifrado ponta a ponta. O servidor vê só IDs, nomes dos computadores, horários e volume de dados.");
                    let tw = ui.painter().layout_no_wrap(text.into(), egui::FontId::proportional(11.5), p.muted).size().x + 20.0;
                    ui.add_space(((w - tw) / 2.0).max(0.0));
                    icons::icon(ui, Icon::Lock, 12.0, p.muted);
                    ui.label(RichText::new(text).size(11.5).color(p.muted));
                });
                ui.add_space(12.0);
            }
            let h = inner.min_rect().height();
            outer.allocate_space(Vec2::new(full.width(), h));
        });
    }

    /// Faixa de atualização na tela inicial. Devolve `true` para reiniciar e atualizar.
    fn update_banner(&mut self, ui: &mut egui::Ui, p: &Palette) -> bool {
        use crate::update::State;
        let mut restart = false;
        let mut download = false;
        let (text, action): (String, Option<&str>) = match &self.updater.state {
            State::Available { manifest, asset: Some(_) } if !self.settings.auto_update => (t!("Nova versão do Elo disponível: {}.", manifest.version), Some(t!("Atualizar")),),
            State::Available { manifest, asset: None } => (t!("Nova versão do Elo disponível: {}.", manifest.version), Some(t!("Baixar no GitHub")),),
            State::Downloading { version, done, total } => {
                let pct = done.load(std::sync::atomic::Ordering::Relaxed) as f32 / (*total).max(1) as f32 * 100.0;
                (t!("Baixando a versão {}… {}%", version, format!("{:.0}", pct)), None)
            }
            State::Ready { version, .. } => (
                t!("A versão {} está pronta.{}", version, if self.views.is_empty() && self.hosts.is_empty() { "" } else { t!(" Instala ao fim das sessões, ou agora (as sessões caem).") }),
                Some(t!("Reiniciar e atualizar")),
            ),
            _ => return false,
        };
        egui::Frame::new().fill(theme::alpha(p.accent, 0.10)).stroke(Stroke::new(1.0, theme::alpha(p.accent, 0.35))).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icons::icon(ui, Icon::Download, 15.0, p.accent_text);
                ui.label(RichText::new(&text).size(13.0).color(p.text));
                if let Some(a) = action {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::primary(ui, None, a, None).clicked() {
                            match &self.updater.state {
                                State::Ready { .. } => restart = true,
                                State::Available { asset: None, .. } => { open_url(crate::update::RELEASES)}
                                _ => download = true,
                            }
                        }
                    });
                }
            });
        });
        ui.add_space(14.0);
        if download {
            self.updater.download_now();
        }
        restart
    }

    /// Cartão azul com o ID deste computador e a senha temporária.
    fn hero(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let white = Color32::WHITE;
        let soft = Color32::from_white_alpha(200);
        let r = egui::Frame::new().fill(p.accent).corner_radius(CornerRadius::same(16)).inner_margin(Margin::same(22)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("SEU ID")).font(theme::semibold(11.0)).extra_letter_spacing(0.8).color(soft));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (t, c) = match &self.status {
                        Status::Online(_) => (t!("Online"), Color32::from_rgb(120, 240, 170)),
                        Status::Connecting => (t!("Conectando"), Color32::from_rgb(255, 214, 120)),
                        Status::Offline(_) => (t!("Sem conexão"), Color32::from_rgb(255, 160, 160)),
                    };
                    egui::Frame::new().fill(Color32::from_white_alpha(38)).corner_radius(CornerRadius::same(255)).inner_margin(Margin::symmetric(9, 3)).show(ui, |ui| {
                        // Dentro do layout da direita para a esquerda: o primeiro fica à direita.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(RichText::new(t).font(theme::medium(11.5)).color(white));
                            theme::dot(ui, c, 7.0);
                        });
                    });
                });
            });
            ui.add_space(6.0);
            match self.status.clone() {
                Status::Online(id) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(pretty_id(&id)).font(theme::mono(40.0)).color(white));
                        ui.add_space(4.0);
                        if light_button(ui, Icon::Copy, t!("Copiar ID")).clicked() {
                            copy(ui, &pretty_id(&id));
                            self.toast(t!("ID copiado"));
                        }
                    });
                }
                Status::Connecting => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(white).size(22.0));
                        ui.label(RichText::new(t!("obtendo ID…")).size(16.0).color(soft));
                    });
                    ui.add_space(14.0);
                }
                Status::Offline(r) => {
                    ui.label(RichText::new("— — —").font(theme::mono(40.0)).color(Color32::from_white_alpha(120)));
                    ui.add(egui::Label::new(RichText::new(t!("Sem conexão com o servidor: {}", r)).size(12.0).color(white)).wrap());
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new(t!("SENHA TEMPORÁRIA")).font(theme::semibold(11.0)).extra_letter_spacing(0.8).color(soft));
            ui.add_space(4.0);
            egui::Frame::new().fill(Color32::from_black_alpha(46)).corner_radius(CornerRadius::same(10)).inner_margin(Margin { left: 14, right: 6, top: 4, bottom: 4 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let shown = if self.show_temp { self.temp_password.clone() } else { "••••••••".into() };
                    ui.label(RichText::new(shown).font(theme::mono(19.0)).color(white));
                    ui.add_space(10.0);
                    if light_button(ui, Icon::Eye, if self.show_temp { t!("Ocultar") } else { t!("Mostrar") }).clicked() {
                        self.show_temp = !self.show_temp;
                    }
                    if light_button(ui, Icon::Copy, t!("Copiar senha")).clicked() {
                        copy(ui, &self.temp_password);
                        self.toast(t!("Senha copiada"));
                    }
                    if light_button(ui, Icon::Refresh, t!("Gerar nova senha")).clicked() {
                        self.new_temp_password();
                        self.toast(t!("Nova senha temporária gerada (a anterior deixa de valer)"));
                    }
                });
            });
            ui.add_space(10.0);
            ui.add(egui::Label::new(RichText::new(t!("Passe o ID e a senha para quem vai ajudar. Sem a senha, cada pedido precisa do seu aceite.")).size(12.0).color(soft)).wrap());
        });
        // Anéis decorativos à direita (longe dos cantos arredondados).
        let rect = r.response.rect;
        let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
        let c = egui::pos2(rect.right() + 40.0, rect.bottom() - rect.height() * 0.28);
        for (rad, a) in [(rect.height() * 0.52, 9u8), (rect.height() * 0.32, 12u8)] {
            painter.circle_stroke(c, rad, Stroke::new(14.0, Color32::from_white_alpha(a)));
        }
    }

    fn unattended_card(&mut self, ui: &mut egui::Ui, p: &Palette) {
        card(p).inner_margin(Margin::symmetric(18, 14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let mut un = self.settings.unattended;
                ui.vertical(|ui| {
                    ui.label(RichText::new(t!("Acesso não assistido")).font(theme::semibold(13.5)).color(p.text));
                    ui.label(RichText::new(if self.settings.unattended { t!("Ligado: a senha fixa entra sem ninguém aqui.") } else { t!("Entrar com uma senha fixa, sem ninguém aqui.") }).size(12.0).color(p.text2));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::switch(ui, &mut un, "").changed() {
                        if un && self.settings.permanent_password.len() < 8 {
                            self.show_settings = true;
                            self.toast(t!("Defina uma senha fixa (8+ caracteres) nas configurações"));
                        } else {
                            self.settings.unattended = un;
                            self.settings.save();
                        }
                    }
                });
            });
        });
    }

    /// Cartão "Acessar outro computador". `min_h`: altura da coluna ao lado.
    fn connect_card(&mut self, ui: &mut egui::Ui, p: &Palette, min_h: f32) {
        card(p).corner_radius(CornerRadius::same(16)).inner_margin(Margin::same(22)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(min_h - 44.0);
            ui.label(RichText::new(t!("Acessar outro computador")).font(theme::semibold(16.0)).color(p.text));
            ui.label(RichText::new(t!("Digite o ID que a outra pessoa passou.")).size(12.5).color(p.text2));
            ui.add_space(16.0);
            theme::field_label(ui, t!("ID do computador"));
            let out = egui::TextEdit::singleline(&mut self.target)
                .hint_text(RichText::new("123 456 789").font(theme::mono(28.0)).color(theme::alpha(p.muted, 0.6)))
                .font(theme::mono(28.0))
                .margin(Margin::symmetric(16, 8))
                .min_size(Vec2::new(0.0, 64.0))
                .align(Align2::LEFT_CENTER)
                .desired_width(f32::INFINITY)
                .show(ui);
            let r = out.response;
            // Formata enquanto digita (e leva o cursor para o fim).
            if r.changed() {
                let d = elo_proto::ctrl::clean_id(&self.target);
                let f = pretty_id(&d.chars().take(9).collect::<String>());
                if f != self.target {
                    self.target = f;
                    let mut st = out.state;
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.target.chars().count()))));
                    st.store(ui.ctx(), r.id);
                }
            }
            ui.add_space(10.0);
            theme::field_label(ui, t!("Senha (opcional)"));
            let saved = self.settings.saved_password(&elo_proto::ctrl::clean_id(&self.target)).is_some();
            let hint = if saved { t!("usando a senha salva no favorito") } else { t!("vazio = pedir permissão na tela") };
            let r2 = ui.add(egui::TextEdit::singleline(&mut self.password).password(true).hint_text(hint).font(egui::FontId::proportional(14.5)).margin(Margin::symmetric(16, 6)).min_size(Vec2::new(0.0, 44.0)).align(Align2::LEFT_CENTER).desired_width(f32::INFINITY));
            let enter = (r.lost_focus() || r2.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(16.0);
            let busy = self.pending_connect.is_some();
            let b = big_button(ui, p, if busy { t!("Conectando…") } else { t!("Conectar") }, !busy);
            if (b.clicked() || enter) && !busy {
                let (t, pw) = (self.target.clone(), self.password.clone());
                self.connect(t, pw);
            }
        });
    }

    /// Favoritos e recentes em cartões. Clique preenche o ID; clique duplo conecta.
    fn computers(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let favs: Vec<(String, String, String, String, bool)> = self.settings.favorites.iter().map(|f| { (f.id.clone(), f.label(), if f.alias.trim().is_empty() || f.name.is_empty() { String::new() } else { f.name.clone() }, f.os.clone(), true)}).collect();
        let recent: Vec<(String, String, String, String, bool)> = self
            .settings
            .recent
            .iter()
            .filter(|r| self.settings.favorite(&r.id).is_none())
            .take(6)
            .map(|r| { (r.id.clone(), if r.name.is_empty() { pretty_id(&r.id) } else { r.name.clone() }, String::new(), r.os.clone(), false)})
            .collect();
        if favs.is_empty() && recent.is_empty() {
            return;
        }
        let mut out = TileOut::default();
        let cols = if ui.available_width() > 780.0 { 3 } else { 2 };
        for (title, list) in [(t!("Favoritos"), favs), (t!("Recentes"), recent)] {
            if list.is_empty() {
                continue;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(theme::semibold(14.0)).color(p.text));
                ui.label(RichText::new(list.len().to_string()).size(12.0).color(p.muted));
            });
            ui.add_space(8.0);
            for chunk in list.chunks(cols) {
                ui.columns(cols, |c| {
                    for (i, (id, name, sub_name, os, fav)) in chunk.iter().enumerate() {
                        tile(&mut c[i], p, id, name, sub_name, os, *fav, &mut out);
                    }
                });
                ui.add_space(10.0);
            }
            ui.add_space(8.0);
        }
        if let Some(id) = out.wake {
            if matches!(self.status, Status::Online(_)) {
                let _ = self.control.tx.send(elo_core::client::Command::Wake { target: id });
            } else {
                self.error(t!("Sem conexão com o servidor do Elo."));
            }
        }
        if let Some(id) = out.start_rename
            && let Some(f) = self.settings.favorite(&id)
        {
            self.renaming = Some(crate::app::FavEdit { id, alias: f.alias.clone(), password: f.password.clone(), show: false, new: false });
        }
        if let Some(id) = out.star {
            if self.settings.favorite(&id).is_some() {
                self.settings.favorites.retain(|f| f.id != id);
            } else if let Some(r) = self.settings.recent.iter().find(|r| r.id == id).cloned() {
                self.settings.toggle_favorite(&r.id, &r.name, &r.os, "");
                self.renaming = Some(crate::app::FavEdit { id, alias: String::new(), password: String::new(), show: false, new: true });
            }
            self.settings.save();
        }
        if let Some((id, now)) = out.go {
            self.target = pretty_id(&id);
            if now {
                let pw = self.password.clone();
                self.connect(id, pw);
            }
        }
    }

    fn remote_view(&mut self, ui: &mut egui::Ui, idx: usize) {
        let ctx = ui.ctx().clone();
        let fav = self.settings.favorite(&self.views[idx].target_id).is_some();
        let fs = self.fullscreen;
        let right = self.settings.bar_right;
        let act = &mut self.session_act;
        let v = &mut self.views[idx];
        // Barra da sessão: cápsula de vidro flutuando no alto ou à direita.
        *act = session_bar(ui, v, idx, fav, fs, right).or(act.take());
        // Barra à direita fora da tela cheia: a imagem não fica embaixo dela.
        let reserve = if right && !fs { BAR_BAND } else { 0.0 };
        if v.confirm_restart {
            let mut close = false;
            let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
            let m = mac::DARK;
            egui::Modal::new(egui::Id::unique("reiniciar")).frame(mac::alert_frame(&m)).show(&ctx, |ui| {
                ui.style_mut().visuals = egui::Visuals::dark();
                ui.set_width(300.0);
                ui.vertical_centered(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
                    mac::icon_tile(ui.painter(), r, Icon::Refresh, m.orange);
                    ui.add_space(10.0);
                    ui.label(RichText::new(t!("Reiniciar {}?", name)).font(theme::semibold(15.0)).color(m.label));
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(t!("A sessão cai e o Elo reconecta sozinho por até 6 minutos. Para voltar sem ninguém lá, o outro computador precisa abrir o Elo junto com o sistema e ter o acesso não assistido ligado.")).size(12.0).color(m.label2)).halign(Align::Center).wrap());
                });
                ui.add_space(18.0);
                ui.columns(2, |c| {
                    let w = Vec2::new(c[0].available_width(), 30.0);
                    if mac::button(&mut c[0], &m, t!("Cancelar"), Kind::Secondary, Some(w), true).clicked() || c[0].input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                    if mac::button(&mut c[1], &m, t!("Reiniciar"), Kind::Destructive, Some(w), true).clicked() {
                        let _ = v.handle.cmd.send(Cmd::Restart);
                        close = true;
                    }
                });
            });
            if close {
                v.confirm_restart = false;
            }
        }
        // Tela remota.
        if !v.connected {
            // Cartão de vidro no meio: quem, situação, código e Cancelar.
            let m = mac::DARK;
            let mut area = ui.available_rect_before_wrap();
            area.max.x -= reserve;
            let h = if v.waiting && !v.sas.is_empty() { 360.0 } else { 250.0 };
            let card = egui::Rect::from_center_size(area.center(), Vec2::new(340.0, h));
            ui.scope_builder(egui::UiBuilder::new().max_rect(card).layout(Layout::top_down(Align::Center)), |ui| {
                glass_frame(22, Margin::same(26)).show(ui, |ui| {
                    ui.style_mut().visuals = egui::Visuals::dark();
                    ui.set_width(288.0);
                    ui.vertical_centered(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        mac::icon_tile(ui.painter(), r, Icon::Monitor, os_color(&v.os));
                        ui.add_space(12.0);
                        let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
                        ui.label(RichText::new(name).font(theme::semibold(16.0)).color(m.label));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let text = if v.waiting { t!("Esperando o outro lado aceitar") } else { t!("Conectando…") };
                            let tw = ui.painter().layout_no_wrap(text.into(), egui::FontId::proportional(13.0), m.label2).size().x + 22.0;
                            ui.add_space(((ui.available_width() - tw) / 2.0).max(0.0));
                            ui.add(egui::Spinner::new().size(14.0).color(m.label2));
                            ui.label(RichText::new(text).size(13.0).color(m.label2));
                        });
                        if v.waiting && !v.sas.is_empty() {
                            ui.add_space(14.0);
                            egui::Frame::new().fill(Color32::from_white_alpha(14)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(18, 8)).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new(t!("Código de verificação")).size(11.0).color(m.label2));
                                    ui.label(RichText::new(&v.sas).font(theme::mono(24.0)).color(m.label));
                                });
                            });
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(RichText::new(t!("Confira se é o mesmo código na tela de quem vai aceitar.")).size(11.5).color(m.label2)).halign(Align::Center).wrap());
                        }
                        ui.add_space(18.0);
                        if mac::button(ui, &m, t!("Cancelar"), Kind::Secondary, Some(Vec2::new(140.0, 30.0)), true).clicked() {
                            v.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                });
            });
            return;
        }
        let (w, h, version) = {
            let r = v.remote.lock().unwrap_or_else(|e| e.into_inner());
            (r.dec.w, r.dec.h, r.version)
        };
        if w == 0 {
            ui.centered_and_justified(|ui| ui.spinner());
            return;
        }
        if v.texture.is_none() || v.tex_version != version {
            let r = v.remote.lock().unwrap_or_else(|e| e.into_inner());
            let img = egui::ColorImage::from_rgba_unmultiplied([r.dec.w as usize, r.dec.h as usize], &r.dec.rgba);
            drop(r);
            match &mut v.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => { v.texture = Some(ctx.load_texture(format!("remota-{idx}"), img, egui::TextureOptions::LINEAR))}
            }
            v.tex_version = version;
        }
        let mut avail = ui.available_rect_before_wrap();
        avail.max.x -= reserve;
        let scale = if v.fit { (avail.width() / w as f32).min(avail.height() / h as f32).min(1.0) } else { 1.0 / ctx.pixels_per_point() };
        let size = Vec2::new(w as f32 * scale, h as f32 * scale);
        let rect = egui::Rect::from_center_size(avail.center(), size);
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());
        if let Some(t) = &v.texture {
            // Como uma janela: cantos arredondados e sombra (some quando ocupa tudo).
            let radius = if rect.width() >= avail.width() - 1.0 && rect.height() >= avail.height() - 1.0 { 0 } else { 10 };
            if radius > 0 {
                ui.painter().add(egui::Shadow { offset: [0, 10], blur: 36, spread: 0, color: Color32::from_black_alpha(150) }.as_shape(rect, CornerRadius::same(radius)));
            }
            egui::Image::from_texture(egui::load::SizedTexture::new(t.id(), rect.size())).corner_radius(CornerRadius::same(radius)).paint_at(ui, rect);
        }
        if !v.permissions.control {
            ui.painter().text(rect.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, t!("Somente visualização (o outro lado desativou o controle)"), egui::FontId::proportional(13.0), Color32::from_rgb(255, 210, 120));
        }
        // Escolhendo a área do print: nada vai para o outro computador.
        if v.shot.is_some() {
            release_remote_input(v, true);
            if let Some(a) = select_shot_area(ui, &resp, v, rect, scale, (w, h)) {
                *act = Some(a);
            }
            return;
        }
        let remote_cursor = v.remote.lock().unwrap_or_else(|e| e.into_inner()).cursor;
        let cmd = v.handle.cmd.clone();
        let send = move |i: Input| {
            let _ = cmd.send(Cmd::Input(i));
        };
        let cmd = v.handle.cmd.clone();
        let paste = move |t: String| {
            let _ = cmd.send(Cmd::PasteText(t));
        };
        remote_input(&ctx, ui, &resp, rect, scale, remote_cursor, &mut v.input, (v.os == "macOS", v.fine_scroll), &send, &paste);
    }

    /// Salva o print da tela remota (inteira ou a área) em PNG na pasta de
    /// recebidos e copia para a área de transferência. Usa a imagem que já
    /// chegou aqui: funciona com qualquer versão do Elo do outro lado.
    fn take_shot(&mut self, idx: usize, area: Option<[u32; 4]>) {
        let Some(v) = self.views.get(idx) else { return };
        let (w, h, rgba) = {
            let r = v.remote.lock().unwrap_or_else(|e| e.into_inner());
            (r.dec.w, r.dec.h, r.dec.rgba.clone())
        };
        if w == 0 || h == 0 {
            self.error(t!("Ainda não chegou imagem do outro computador."));
            return;
        }
        let [x, y, cw, ch] = shot_crop(area, w, h);
        let name = if v.target_name.is_empty() { v.target_id.clone() } else { v.target_name.clone() };
        let dir = self.settings.downloads.clone();
        let tx = self.shots.0.clone();
        let clipboard = self.shot_clipboard.clone();
        std::thread::Builder::new()
            .name("elo-print".into())
            .spawn(move || {
                let r = (|| {
                    let full = image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "imagem incompleta".to_string())?;
                    let mut img = image::imageops::crop_imm(&full, x, y, cw, ch).to_image();
                    // A tela não tem transparência (alguns sistemas mandam o alfa zerado).
                    img.pixels_mut().for_each(|p| p.0[3] = 255);
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let path = shot_path(&dir, &name);
                    img.save_with_format(&path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    let mut cb = clipboard.lock().unwrap_or_else(|e| e.into_inner());
                    if cb.is_none() {
                        *cb = arboard::Clipboard::new().ok();
                    }
                    let copied = cb.as_mut().is_some_and(|c| c.set_image(arboard::ImageData { width: cw as usize, height: ch as usize, bytes: std::borrow::Cow::Borrowed(img.as_raw()) }).is_ok());
                    Ok((path, copied))
                })();
                let _ = tx.send(r);
            })
            .ok();
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.clients_dialogs(ctx);
        self.fav_editor(ctx);
        // Pedidos de acesso (sem senha).
        if let Some(a) = self.asks.first_mut() {
            let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
            let mut decision: Option<bool> = None;
            let remaining = 90u64.saturating_sub(a.at.elapsed().as_secs());
            egui::Modal::new(egui::Id::unique("ask")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Shield, 20.0, p.warning);
                    ui.label(RichText::new(t!("Pedido de acesso")).font(theme::semibold(17.0)).color(p.text));
                });
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(t!("{} (ID {}) quer acessar este computador.", a.from_name, pretty_id(&a.from_id))).size(13.5).color(p.text)).wrap());
                if a.verified {
                    ui.horizontal(|ui| {
                        icons::icon(ui, Icon::Check, 14.0, p.success);
                        ui.label(RichText::new(t!("Técnico aprovado pela empresa (identidade conferida pela chave dos técnicos)")).size(12.0).color(p.success));
                    });
                }
                ui.add_space(6.0);
                ui.label(RichText::new(t!("Código de verificação: {}", a.sas)).font(theme::mono(15.0)).color(p.text));
                ui.label(RichText::new(t!("Confirme com a pessoa que o código aparece igual na tela dela. Só aceite se você conhece quem está pedindo.")).size(11.5).color(p.text2));
                ui.add_space(10.0);
                ui.label(RichText::new(t!("Permitir")).size(12.0).color(p.muted));
                ui.checkbox(&mut a.permissions.control, t!("Controlar mouse e teclado"));
                ui.checkbox(&mut a.permissions.clipboard, t!("Área de transferência"));
                ui.checkbox(&mut a.permissions.files, t!("Transferir arquivos"));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::primary(ui, None, t!("Aceitar"), None).clicked() {
                        decision = Some(true);
                    }
                    if theme::secondary(ui, None, t!("Recusar")).clicked() {
                        decision = Some(false);
                    }
                    ui.label(RichText::new(t!("{} s", remaining)).size(11.5).color(p.muted));
                });
            });
            if remaining == 0 {
                decision = Some(false);
            }
            if let Some(ok) = decision {
                let a = self.asks.remove(0);
                let _ = a.reply.send(ok.then_some(a.permissions));
                if let Some(h) = self.hosts.iter_mut().find(|h| h.session == a.session) {
                    h.permissions = a.permissions;
                }
            }
        }
        if self.show_settings && self.panel.is_none() {
            self.settings_dialog(ctx);
        }
        if self.panel.is_some() {
            self.server_panel(ctx);
        }
    }

    /// Painel do servidor embutido: online, sessões em andamento e histórico.
    /// Editar favorito: nome dado aqui e senha do acesso não assistido de lá.
    fn fav_editor(&mut self, ctx: &egui::Context) {
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        let Some(e) = self.renaming.as_mut() else { return ; };
        let Some(f) = self.settings.favorite(&e.id) else {
            self.renaming = None;
            return;
        };
        let reported = if f.name.is_empty() { pretty_id(&f.id) } else { format!("{} · {}", f.name, pretty_id(&f.id)) };
        let (mut save, mut close) = (false, false);
        egui::Modal::new(egui::Id::unique("favorito")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                icons::icon(ui, Icon::Star, 18.0, p.warning);
                ui.label(RichText::new(if e.new { t!("Salvo nos favoritos") } else { t!("Editar favorito") }).font(theme::semibold(16.0)).color(p.text));
            });
            ui.label(RichText::new(reported).font(theme::mono(12.0)).color(p.muted));
            ui.add_space(12.0);
            theme::field_label(ui, t!("Nome"));
            let r = ui.add(egui::TextEdit::singleline(&mut e.alias).hint_text(if f.name.is_empty() { t!("Nome para este computador").to_string() } else { f.name.clone() }).desired_width(f32::INFINITY));
            if e.new && r.ctx.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            ui.add_space(8.0);
            theme::field_label(ui, t!("Senha do acesso não assistido"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut e.password).password(!e.show).hint_text(t!("vazio = pedir permissão na tela")).desired_width(ui.available_width() - 70.0));
                if ui.selectable_label(e.show, if e.show { t!("Ocultar") } else { t!("Mostrar") }).clicked() {
                    e.show = !e.show;
                }
            });
            ui.add(egui::Label::new(RichText::new(t!("A senha fixa configurada no outro computador. Com ela salva, o Elo conecta direto, sem ninguém precisar aceitar lá. Fica guardada só neste computador.")).size(11.5).color(p.text2)).wrap());
            ui.add_space(14.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary(ui, None, t!("Salvar"), None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        if save {
            let e = self.renaming.take().expect("editando");
            if let Some(f) = self.settings.favorites.iter_mut().find(|f| f.id == e.id) {
                f.alias = e.alias.trim().to_string();
                f.password = e.password;
            }
            self.settings.save();
        } else if close {
            self.renaming = None;
        }
    }

    fn server_panel(&mut self, ctx: &egui::Context) {
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        // Atualiza a cada 2 s.
        if let (Some((at, _)), Some(srv)) = (&self.panel, &self.local.server)
            && at.elapsed().as_secs() >= 2
        {
            self.panel = Some((std::time::Instant::now(), srv.stats()));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        let Some((_, st)) = &self.panel else { return };
        let mut close = false;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let ago = |t: u64| -> String {
            let d = now.saturating_sub(t);
            match d {
                0..60 => t!("agora").into(),
                60..3600 => t!("há {} min", d / 60),
                3600..86_400 => t!("há {} h", d / 3600),
                _ => t!("há {} dias", d / 86_400),
            }
        };
        let dur = |s: u64| -> String { if s < 60 { t!("{} s", s) } else if s < 3600 { t!("{} min", s / 60) } else { t!("{} h {} min", s / 3600, format!("{:02}", (s % 3600) / 60)) } };
        let who = |name: &str, id: &str| { if name.is_empty() { pretty_id(id) } else { t!("{} ({})", name, pretty_id(id)) } };
        let max_h = (ctx.content_rect().height() - 160.0).max(240.0);
        egui::Modal::new(egui::Id::unique("painel")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(22))).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.horizontal(|ui| {
                icons::icon(ui, Icon::Globe, 18.0, p.accent);
                ui.label(RichText::new(t!("Painel do servidor")).font(theme::semibold(17.0)).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(ui, Icon::Close, t!("Fechar"), false).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (n, label) in [(st.online.len(), t!("online agora")), (st.active.len(), t!("sessões em andamento")), (st.recent.len(), t!("no histórico recente"))] {
                    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(14, 8)).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(n.to_string()).font(theme::semibold(20.0)).color(p.text));
                            ui.label(RichText::new(label).size(11.5).color(p.muted));
                        });
                    });
                }
            });
            ui.add_space(12.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                ui.label(RichText::new(t!("Online agora")).font(theme::semibold(13.5)).color(p.text));
                egui::Grid::new("online").num_columns(4).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                    for h in [t!("Computador"), t!("Sistema"), "IP", t!("Conectado")] {
                        ui.label(RichText::new(h).size(11.5).color(p.muted));
                    }
                    ui.end_row();
                    for d in &st.online {
                        ui.label(RichText::new(who(&d.name, &d.id)).size(12.5).color(p.text));
                        ui.label(RichText::new(&d.os).size(12.0).color(p.text2));
                        ui.label(RichText::new(&d.ip).font(theme::mono(11.5)).color(p.text2));
                        ui.label(RichText::new(ago(d.since)).size(12.0).color(p.text2));
                        ui.end_row();
                    }
                });
                ui.add_space(14.0);
                ui.label(RichText::new(t!("Sessões em andamento")).font(theme::semibold(13.5)).color(p.text));
                if st.active.is_empty() {
                    ui.label(RichText::new(t!("Nenhuma agora.")).size(12.0).color(p.muted));
                }
                for x in &st.active {
                    ui.label(RichText::new(t!("{} acessando {} · começou {}", who(&x.viewer_name, &x.viewer_id), who(&x.host_name, &x.host_id), ago(x.start))).size(12.5).color(p.text));
                }
                ui.add_space(14.0);
                ui.label(RichText::new(t!("Histórico")).font(theme::semibold(13.5)).color(p.text));
                if st.recent.is_empty() {
                    ui.label(RichText::new(t!("Nenhuma sessão registrada.")).size(12.0).color(p.muted));
                }
                egui::Grid::new("historico").num_columns(4).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                    for h in [t!("Quem acessou"), t!("Computador acessado"), t!("Quando"), t!("Duração · volume")] {
                        ui.label(RichText::new(h).size(11.5).color(p.muted));
                    }
                    ui.end_row();
                    for x in &st.recent {
                        ui.label(RichText::new(who(&x.viewer_name, &x.viewer_id)).size(12.0).color(p.text));
                        ui.label(RichText::new(who(&x.host_name, &x.host_id)).size(12.0).color(p.text));
                        ui.label(RichText::new(ago(x.start)).size(12.0).color(p.text2));
                        ui.label(RichText::new(format!("{} · {}", dur(x.end.saturating_sub(x.start)), human_bytes(x.bytes))).size(12.0).color(p.text2));
                        ui.end_row();
                    }
                });
            });
            ui.add_space(10.0);
            ui.label(RichText::new(if st.retention_days == 0 { t!("O histórico é guardado para sempre. Defina um prazo nas configurações (LGPD).").to_string() } else { t!("O histórico é apagado depois de {} dias. Nomes aparecem para quem se conectou desde que o servidor abriu.", st.retention_days) }).size(11.5).color(p.muted));
        });
        if close {
            self.panel = None;
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        let mut close = false;
        let mut test = false;
        let mut save = false;
        let mut wizard = false;
        let mut renamed = false;
        let mut update_go = false;
        let mut update_restart = false;
        let mut open_panel = false;
        self.poll_probe();
        if self.probe.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let max_h = (ctx.content_rect().height() - 150.0).max(240.0);
        egui::Modal::new(egui::Id::unique("settings")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(RichText::new(t!("Configurações")).font(theme::semibold(17.0)).color(p.text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            theme::field_label(ui, t!("Idioma"));
            let langs = [("auto", t!("Automático (o do sistema)")), ("pt", "Português"), ("en", "English")];
            let current = langs.iter().find(|(k, _)| *k == self.settings.language).map(|(_, n)| *n).unwrap_or(langs[0].1);
            let mut lang = self.settings.language.clone();
            egui::ComboBox::from_id_salt("idioma").selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                for (k, n) in langs {
                    ui.selectable_value(&mut lang, k.to_string(), n);
                }
            });
            if lang != self.settings.language {
                // Vale na hora (e fica salvo mesmo se cancelar o resto).
                self.settings.language = lang;
                crate::i18n::set(&self.settings.language);
                let mut disk = crate::settings::Settings::load();
                disk.language = self.settings.language.clone();
                disk.save();
            }
            ui.add_space(6.0);
            theme::field_label(ui, t!("Nome deste computador"));
            ui.add(egui::TextEdit::singleline(&mut self.settings.device_name).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.checkbox(&mut self.autostart, t!("Iniciar o Elo junto com o computador")).on_hover_text(t!("Abre na bandeja ao entrar no sistema, para continuar acessível depois de reiniciar."));
            ui.add_enabled_ui(self.tray.is_some(), |ui| {
                ui.checkbox(&mut self.settings.close_to_tray, t!("Ao fechar a janela, continuar rodando na bandeja"))
                    .on_hover_text(t!("O Elo fica perto do relógio e continua aceitando conexões. Para sair, use \"Sair do Elo\" no menu do ícone."))
                    .on_disabled_hover_text(t!("A bandeja do sistema não está disponível (no GNOME, precisa da extensão AppIndicator)."));
            });
            let client = self.client_mode();
            if !client {
                ui.add_space(6.0);
                theme::field_label(ui, t!("Senha fixa (acesso não assistido)"));
                ui.add(egui::TextEdit::singleline(&mut self.new_permanent).password(true).hint_text(if self.settings.permanent_password.is_empty() { t!("mínimo de 8 caracteres") } else { t!("•••••••• (digite para trocar)") }).desired_width(f32::INFINITY));
                if !self.new_permanent.is_empty() && self.new_permanent.len() < 8 {
                    ui.label(RichText::new(t!("Use pelo menos 8 caracteres.")).size(11.5).color(p.danger));
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t!("Permissões padrão para quem acessa")).size(12.0).color(p.muted));
            ui.checkbox(&mut self.settings.default_permissions.control, t!("Controlar mouse e teclado"));
            ui.checkbox(&mut self.settings.default_permissions.clipboard, t!("Área de transferência"));
            ui.checkbox(&mut self.settings.default_permissions.files, t!("Transferir arquivos"));
            ui.add_space(6.0);
            theme::field_label(ui, t!("Arquivos recebidos em"));
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.settings.downloads.display().to_string()).font(theme::mono(11.5)).color(p.text2));
                if ui.small_button(t!("Escolher…")).clicked()
                    && let Some(d) = rfd::FileDialog::new().pick_folder()
                {
                    self.settings.downloads = d;
                }
            });
            ui.add_space(6.0);
            theme::field_label(ui, t!("Barra durante o acesso"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.settings.bar_right, false, t!("No alto"));
                ui.radio_value(&mut self.settings.bar_right, true, t!("Na lateral direita"));
            });
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new(t!("Atualizações")).font(theme::semibold(13.0)).color(p.text));
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Versão instalada: {}", crate::update::CURRENT)).size(12.5).color(p.text2));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!self.updater.busy(), egui::Button::new(t!("Verificar agora"))).clicked() {
                        self.updater.check_now();
                    }
                });
            });
            {
                use crate::update::State;
                match &self.updater.state {
                    State::Idle => {}
                    State::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(t!("Verificando…")).size(12.0).color(p.text2));
                        });
                    }
                    State::UpToDate => {
                        ui.label(RichText::new(t!("Você está na versão mais recente.")).size(12.0).color(p.success));
                    }
                    State::Available { manifest, asset } => {
                        let v = manifest.version.clone();
                        let has = asset.is_some();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("Versão {} disponível.", v)).size(12.0).color(p.accent_text));
                            if has {
                                if ui.small_button(t!("Baixar e instalar")).clicked() {
                                    update_go = true;
                                }
                            } else if ui.small_button(t!("Abrir página de download")).clicked() {
                                open_url(crate::update::RELEASES);
                            }
                        });
                    }
                    State::Downloading { version, done, total } => {
                        let f = done.load(std::sync::atomic::Ordering::Relaxed) as f32 / (*total).max(1) as f32;
                        ui.label(RichText::new(t!("Baixando a versão {}…", version)).size(12.0).color(p.text2));
                        ui.add(egui::ProgressBar::new(f).desired_height(5.0));
                    }
                    State::Ready { version, .. } => {
                        let v = version.clone();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t!("Versão {} pronta para instalar.", v)).size(12.0).color(p.success));
                            if ui.small_button(t!("Reiniciar e atualizar")).clicked() {
                                update_restart = true;
                            }
                        });
                    }
                    State::Failed(e) => {
                        ui.add(egui::Label::new(RichText::new(t!("Não foi possível atualizar: {}", e)).size(12.0).color(p.danger)).wrap());
                    }
                }
            }
            ui.checkbox(&mut self.settings.auto_update, t!("Atualizar automaticamente")).on_hover_text(t!("Baixa sozinho e instala quando não há sessão em andamento. O Elo reabre em seguida."));
            if let crate::update::Install::Manual(why) = &self.updater.install {
                ui.add(egui::Label::new(RichText::new(t!("Este Elo não se atualiza sozinho: {}.", why)).size(11.5).color(p.muted)).wrap());
            }
            ui.add_space(8.0);
            ui.separator();
            if client {
                ui.label(RichText::new(t!("Empresa")).font(theme::semibold(13.0)).color(p.text));
                let name = self.settings.company.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| t!("nenhuma ainda").into());
                ui.label(RichText::new(t!("Este computador é atendido como {}. Só os técnicos da empresa conseguem pedir acesso, e você sempre aceita na tela.", name)).size(12.0).color(p.text2));
                egui::CollapsingHeader::new(RichText::new(t!("Usar outro código de convite")).size(12.0).color(p.text2)).id_salt("outro-convite").show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.invite_input).desired_rows(2).font(theme::mono(11.5)).hint_text("ELO2-…").desired_width(f32::INFINITY));
                    let decoded = elo_proto::ctrl::Invite::decode(&self.invite_input);
                    if let Some(i) = &decoded {
                        crate::empresa::invite_summary(ui, &p, i);
                    }
                    if ui.add_enabled(decoded.is_some(), egui::Button::new(t!("Usar este código"))).clicked() {
                        let text = std::mem::take(&mut self.invite_input);
                        if let Err(e) = self.join_invite(&text) {
                            self.error(e);
                        }
                    }
                });
            } else {
            self.tech_section(ui, &p);
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new(t!("Servidor")).font(theme::semibold(13.0)).color(p.text));
            ui.checkbox(&mut self.settings.local_server.enabled, t!("Rodar o servidor neste computador"));
            if self.settings.local_server.enabled {
                let probing = self.probe.is_some();
                let probe_result = self.probe_result.clone();
                let local_running = self.local.server.is_some();
                let ls = &mut self.settings.local_server;
                ui.horizontal(|ui| {
                    theme::field_label(ui, t!("Porta"));
                    ui.add(egui::DragValue::new(&mut ls.port).range(1024..=65535).speed(0.0));
                    ui.add_space(12.0);
                    theme::field_label(ui, t!("Histórico"));
                    ui.add(egui::DragValue::new(&mut ls.retention_days).range(0..=3650).suffix(t!(" dias")).speed(1.0)).on_hover_text(t!("Por quantos dias o servidor guarda o histórico de sessões (quem acessou quem, quando e quanto). 0 = para sempre. Esses dados são pessoais (LGPD)."));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(local_running, egui::Button::new(t!("Painel do servidor"))).on_hover_text(t!("Quem está online, sessões em andamento e histórico")).clicked() {
                            open_panel = true;
                        }
                    });
                });
                ui.checkbox(&mut ls.lan, t!("Aceitar conexões diretas da rede")).on_hover_text(t!("Desligado, só aceita conexões deste computador (ex.: cloudflared instalado aqui), sem deixar a porta aberta na rede. Com o cloudflared no Docker, deixe ligado."));
                theme::field_label(ui, t!("Endereço público (opcional)"));
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 70.0;
                    ui.add(egui::TextEdit::singleline(&mut ls.public).font(theme::mono(12.0)).hint_text("wss://elo.seudominio.com (Cloudflare Tunnel)").desired_width(w));
                    let can = ls.public_ws().is_some() && !probing;
                    if ui.add_enabled(can, egui::Button::new(if probing { t!("Testando…") } else { t!("Testar") })).on_hover_text(t!("Conecta pelo endereço público e confere se ele chega a este servidor.")).clicked() {
                        test = true;
                    }
                });
                if !ls.public.trim().is_empty() && ls.public_ws().is_none() {
                    ui.label(RichText::new(t!("Comece com wss:// (o túnel do Cloudflare fala WebSocket).")).size(11.5).color(p.danger));
                }
                if let Some((ok, msg)) = &probe_result {
                    ui.horizontal(|ui| {
                        icons::icon(ui, if *ok { Icon::Check } else { Icon::Alert }, 13.0, if *ok { p.success } else { p.danger });
                        ui.add(egui::Label::new(RichText::new(msg).size(11.5).color(if *ok { p.success } else { p.danger })).wrap());
                    });
                }
                cloudflare_help(ui, &p, ls.port);
                if ls.same_bind(&self.local.started) {
                    if let Some(e) = &self.local.error {
                        ui.label(RichText::new(t!("Não foi possível iniciar: {}", e)).size(11.5).color(p.danger));
                    }
                    ui.label(RichText::new(t!("Nos outros computadores, desligue \"Rodar o servidor\" e use:")).size(11.5).color(p.muted));
                    let mut rows: Vec<(&str, String)> = Vec::new();
                    if let Some(w) = ls.public_ws() {
                        rows.push((t!("pela internet (sem impressão digital)"), w.to_string()));
                    }
                    if ls.lan {
                        rows.push((t!("na rede local (TLS ligado)"), self.local.lan_address()));
                        if !self.local.fingerprint.is_empty() {
                            rows.push((t!("impressão digital (só na rede local)"), self.local.fingerprint.clone()));
                        }
                    }
                    if rows.is_empty() {
                        ui.label(RichText::new(t!("Só este computador consegue conectar. Preencha o endereço público do túnel ou ligue as conexões da rede.")).size(11.5).color(p.warning));
                    }
                    for (what, v) in rows {
                        ui.label(RichText::new(what).size(11.0).color(p.muted));
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&v).font(theme::mono(11.0)).color(p.text2)).wrap());
                            if ui.small_button(t!("Copiar")).clicked() {
                                copy(ui, &v);
                            }
                        });
                    }
                } else {
                    ui.label(RichText::new(t!("O servidor reinicia ao salvar (sessões em andamento caem).")).size(11.5).color(p.muted));
                }
            } else {
                theme::field_label(ui, t!("Endereço"));
                ui.add(egui::TextEdit::singleline(&mut self.settings.server.address).font(theme::mono(12.0)).hint_text(t!("host:porta ou wss://dominio (Cloudflare Tunnel)")).desired_width(f32::INFINITY));
                let ws = self.settings.server.address.trim().starts_with("wss://") || self.settings.server.address.trim().starts_with("ws://");
                if ws {
                    ui.label(RichText::new(t!("WebSocket: o TLS é o do endereço (wss://). Deixe a impressão digital vazia para Cloudflare.")).size(11.5).color(p.muted));
                } else {
                    ui.checkbox(&mut self.settings.server.tls, t!("TLS (recomendado)"));
                }
                theme::field_label(ui, t!("Impressão digital do certificado (servidor próprio autoassinado)"));
                ui.add(egui::TextEdit::singleline(&mut self.settings.server.pin).font(theme::mono(11.0)).hint_text(t!("vazio = certificado público (Let's Encrypt, Cloudflare etc.)")).desired_width(f32::INFINITY));
            }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t!("O servidor registra só metadados (IDs, nomes dos computadores, horários e volume). Telas, teclas, arquivos e chat são cifrados ponta a ponta com uma chave que só os dois computadores têm.")).size(11.5).color(p.muted));
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new(t!("Sobre")).font(theme::semibold(13.0)).color(p.text));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let tex = stringari_symbol(ui.ctx());
                ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(30.0)));
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{} by Stringari", crate::variant::app_name())).font(theme::semibold(12.5)).color(p.text));
                    ui.label(RichText::new("Stringari Consultoria & Tech").size(11.5).color(p.muted));
                });
            });
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(t!("O Elo é software livre (licença AGPL 3.0) e vem sem nenhuma garantia. O código está no GitHub: sugestões, problemas e colaborações são bem-vindos.")).size(12.0).color(p.text2)).wrap());
            if ui.link(RichText::new("github.com/Stringari-Consultoria/elo").font(theme::mono(11.5))).on_hover_text(REPO).clicked() {
                open_url(REPO);
            }
            });
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary(ui, None, t!("Salvar"), None).clicked() {
                    save = true;
                    renamed = crate::settings::Settings::load().device_name != self.settings.device_name;
                    if self.new_permanent.len() >= 8 {
                        self.settings.permanent_password = std::mem::take(&mut self.new_permanent);
                        self.settings.unattended = true;
                    }
                    self.settings.save();
                    self.reconnect_server();
                    close = true;
                }
                if theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.settings = crate::settings::Settings::load();
                    self.autostart = crate::autostart::enabled();
                    self.new_permanent.clear();
                    close = true;
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if !self.client_mode() && theme::ghost(ui, None, t!("Assistente inicial")).on_hover_text(t!("Abre de novo as boas-vindas: nome, servidor e início automático.")).clicked() {
                        wizard = true;
                    }
                });
            });
        });
        if test {
            self.test_public();
        }
        if save {
            if self.autostart != crate::autostart::enabled()
                && let Err(e) = crate::autostart::set(self.autostart)
            {
                self.error(t!("Iniciar com o computador: {}", e));
            }
            self.autostart = crate::autostart::enabled();
            // O nome só vai ao servidor ao conectar.
            if renamed {
                self.restart_control();
            }
        }
        if update_go {
            self.updater.download_now();
        }
        if open_panel {
            self.panel = self.local.server.as_ref().map(|s| (std::time::Instant::now(), s.stats()));
        }
        if update_restart {
            self.settings.save();
            self.update_and_restart();
        }
        if wizard {
            self.settings = crate::settings::Settings::load();
            self.onboarding = Some(crate::onboarding::Onboarding::new(&self.settings, self.autostart));
            self.probe_result = None;
            close = true;
        }
        if close {
            self.show_settings = false;
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts")).order(egui::Order::Foreground).anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -40.0)).show(ctx, |ui| {
            let p = pal(ui);
            for t in self.toasts.iter().rev().take(4) {
                egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(12, 8)).shadow(ui.visuals().popup_shadow).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.horizontal(|ui| {
                        icons::icon(ui, if t.error { Icon::Alert } else { Icon::Check }, 15.0, if t.error { p.danger } else { p.success });
                        ui.add(egui::Label::new(RichText::new(&t.text).size(12.5).color(p.text)).wrap());
                    });
                });
                ui.add_space(6.0);
            }
        });
    }
}

/// Passo a passo do Cloudflare Tunnel (recolhível), com os valores para copiar.
pub(crate) fn cloudflare_help(ui: &mut egui::Ui, p: &Palette, port: u16) {
    egui::CollapsingHeader::new(RichText::new(t!("Como publicar pela internet (Cloudflare Tunnel)")).size(12.5).color(p.text2)).id_salt("cloudflare").show(ui, |ui| {
        let text = |ui: &mut egui::Ui, t: &str| {
            ui.add(egui::Label::new(RichText::new(t).size(11.5).color(p.text2)).wrap());
        };
        let value = |ui: &mut egui::Ui, label: &str, v: &str| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.5).color(p.muted));
                ui.label(RichText::new(v).font(theme::mono(11.5)).color(p.text));
                if ui.small_button(t!("Copiar")).clicked() {
                    copy(ui, v);
                }
            });
        };
        text(ui, t!("1. No painel da Cloudflare (Zero Trust → Networks → Tunnels), abra o túnel e adicione uma rota do tipo Published application."));
        text(ui, t!("2. Subdomínio e domínio à sua escolha (ex.: elo.seudominio.com). Path: vazio."));
        text(ui, t!("3. Service, se o cloudflared roda direto neste computador:"));
        value(ui, t!("   Type HTTP · URL"), &format!("localhost:{port}"));
        text(ui, t!("   Se o cloudflared roda no Docker, \"localhost\" é o contêiner. Use Type HTTPS com o IP do gateway da rede do contêiner e, em Additional application settings → TLS, ligue No TLS Verify. Deixe \"Aceitar conexões diretas da rede\" ligado."));
        let gws = docker_gateways();
        if (1..=3).contains(&gws.len()) {
            for g in &gws {
                value(ui, t!("   Type HTTPS · URL"), &format!("{g}:{port}"));
            }
            text(ui, t!("   (gateways das redes Docker deste computador: use o da rede do contêiner do cloudflared)"));
        } else {
            if !gws.is_empty() {
                text(ui, &t!("   Gateways das redes Docker deste computador: {}.", gws.join(", ")));
            }
            text(ui, &t!("   Para saber o da rede do cloudflared: docker inspect <contêiner> e procure \"Gateway\". A URL fica <gateway>:{}.", port));
        }
        text(ui, t!("4. Preencha o Endereço público acima com wss:// e o seu endereço, e clique em Testar."));
        text(ui, t!("5. Nos outros computadores: desligue \"Rodar o servidor neste computador\" e use o mesmo wss://, com a impressão digital vazia."));
    });
}

/// Gateways das redes Docker (Linux): o computador visto de dentro dos
/// contêineres. Lê as rotas das interfaces docker0/br-*; o gateway é o
/// primeiro endereço da rede.
fn docker_gateways() -> Vec<String> {
    let Ok(t) = std::fs::read_to_string("/proc/net/route") else { return Vec::new() ; };
    let mut v: Vec<String> = t
        .lines()
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            let (iface, dest, mask) = (*c.first()?, *c.get(1)?, *c.get(7)?);
            if !(iface == "docker0" || iface.starts_with("br-")) || mask == "00000000" {
                return None;
            }
            let d = u32::from_str_radix(dest, 16).ok()?.to_le_bytes();
            Some(std::net::Ipv4Addr::new(d[0], d[1], d[2], d[3].wrapping_add(1)).to_string())
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Repositório do Elo (código aberto).
pub(crate) const REPO: &str = "https://github.com/Stringari-Consultoria/elo";

/// "by {S} Stringari" ao lado do nome do app.
fn by_stringari(ui: &mut egui::Ui, p: &theme::Palette) {
    let tex = stringari_symbol(ui.ctx());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new("by").size(11.0).color(p.muted));
        ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(15.0)));
        ui.label(RichText::new("Stringari").font(theme::semibold(11.5)).color(p.text2));
    })
    .response
    .on_hover_text("Stringari Consultoria & Tech");
}

/// O símbolo da Stringari (carregado uma vez por janela).
pub(crate) fn stringari_symbol(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("marca-stringari");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(include_bytes!("../../../assets/marca/stringari-simbolo.png")).map(|i| i.to_rgba8()).unwrap_or_default();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let t = ctx.load_texture("marca-stringari", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Abre um endereço no navegador.
pub(crate) fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Abre um arquivo ou pasta no programa padrão do sistema.
pub(crate) fn open_path(p: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

/// Junta pasta e nome no formato do computador remoto (\ no Windows, / nos outros).
fn remote_join(dir: &str, name: &str) -> String {
    let sep = if dir.contains('\\') && !dir.contains('/') { '\\' } else { '/' };
    if dir.ends_with(['/', '\\']) { format!("{dir}{name}") } else { format!("{dir}{sep}{name}") }
}

/// Gerenciador de arquivos: pastas do computador acessado, baixar e enviar.
fn files_panel(ui: &mut egui::Ui, m: &mac::Mac, v: &mut ViewSession, downloads: &std::path::Path) {
    let cmd = v.handle.cmd.clone();
    let mut go: Option<String> = None;
    // Cabeçalho.
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(t!("Arquivos")).font(theme::semibold(14.0)).color(m.label));
            let who = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
            ui.label(RichText::new(t!("em {}", who)).size(11.5).color(m.label2));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if mac::round_icon(ui, m, Icon::Close, t!("Fechar"), 26.0, true).clicked() {
                v.files.open = false;
            }
        });
    });
    ui.add_space(10.0);
    let f = &mut v.files;
    let current = f.listing.as_ref().map(|l| l.path.clone()).unwrap_or_default();
    let in_drives = current == elo_proto::msg::DRIVES;
    // Acima da raiz (/ ou C:\) ficam as unidades.
    let parent = f.listing.as_ref().and_then(|l| l.parent.clone()).or_else(|| (f.drives && f.listing.is_some() && !in_drives).then(|| elo_proto::msg::DRIVES.to_string()));
    // Barra: voltar, início, atualizar e o caminho em migalhas (clique duplo edita).
    let edit_id = ui.scope_id().with("editar-caminho");
    let mut editing = ui.ctx().data(|d| d.get_temp::<bool>(edit_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if mac::round_icon(ui, m, Icon::ArrowUp, t!("Pasta de cima"), 28.0, parent.is_some()).clicked() {
            go = parent.clone();
        }
        if mac::round_icon(ui, m, Icon::Home, t!("Pasta pessoal"), 28.0, true).clicked() {
            go = Some(String::new());
        }
        if f.drives && mac::round_icon(ui, m, Icon::Database, t!("Unidades e discos"), 28.0, true).clicked() {
            go = Some(elo_proto::msg::DRIVES.into());
        }
        if mac::round_icon(ui, m, Icon::Refresh, t!("Atualizar"), 28.0, true).clicked() {
            go = Some(current.clone());
        }
        ui.add_space(6.0);
        let (bar, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
        ui.painter().rect(bar, CornerRadius::same(8), Color32::from_white_alpha(10), Stroke::new(1.0, m.sep), egui::StrokeKind::Inside);
        if editing {
            let r = ui
                .scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(10.0, 0.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.path).frame(egui::Frame::NONE).font(theme::mono(12.0)).text_color(m.label).desired_width(f32::INFINITY))
                })
                .inner;
            r.request_focus();
            if r.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = Some(f.path.clone());
                }
                editing = false;
            }
        } else {
            // Migalhas: cada pasta do caminho é clicável; corta o começo se não couber.
            let win = current.contains('\\');
            let sep = if win { '\\' } else { '/' };
            let parts: Vec<&str> = current.split(sep).filter(|x| !x.is_empty()).collect();
            let mut crumbs: Vec<(String, String)> = Vec::new();
            if f.drives {
                crumbs.push((t!("Unidades").to_string(), elo_proto::msg::DRIVES.into()));
            }
            if !win && !in_drives {
                crumbs.push(("/".into(), "/".into()));
            }
            let n = if in_drives { 0 } else { parts.len() };
            for i in 0..n {
                let path = if win { if i == 0 { format!("{}\\", parts[0]) } else { parts[..=i].join("\\") } } else { format!("/{}", parts[..=i].join("/")) };
                crumbs.push((parts[i].to_string(), path));
            }
            let font = egui::FontId::proportional(12.5);
            let chev = 14.0;
            let ws: Vec<f32> = crumbs.iter().map(|(n, _)| { ui.painter().layout_no_wrap(n.clone(), font.clone(), m.label).size().x + 8.0}).collect();
            let room = bar.width() - 16.0;
            let mut first = 0;
            while first + 1 < crumbs.len() && ws[first..].iter().sum::<f32>() + chev * (crumbs.len() - first) as f32 + 20.0 > room {
                first += 1;
            }
            let mut x = bar.left() + 8.0;
            if first > 0 {
                ui.painter().text(egui::pos2(x, bar.center().y), Align2::LEFT_CENTER, "…", font.clone(), m.label3);
                x += 14.0;
            }
            for (i, (name, path)) in crumbs.iter().enumerate().skip(first) {
                if i > first || first > 0 {
                    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + chev / 2.0 - 2.0, bar.center().y), Vec2::splat(10.0)), Icon::ChevronRight, m.label3);
                    x += chev;
                }
                let r = egui::Rect::from_min_size(egui::pos2(x, bar.top() + 3.0), Vec2::new(ws[i], 22.0));
                let cr = ui.interact(r, ui.scope_id().with(("migalha", i)), Sense::click());
                let last = i + 1 == crumbs.len();
                if cr.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(5), m.hover);
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, name, if last { theme::medium(12.5) } else { font.clone() }, if last { m.label } else { m.label2 });
                if cr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !last {
                    go = Some(path.clone());
                }
                x += ws[i];
            }
            if resp.on_hover_text(t!("Clique duplo para digitar um caminho")).double_clicked() {
                f.path = current.clone();
                editing = true;
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(edit_id, editing));
    ui.add_space(8.0);
    if let Some(e) = f.listing.as_ref().and_then(|l| l.error.clone()) {
        ui.add(egui::Label::new(RichText::new(e).size(12.0).color(m.red)).wrap());
        ui.add_space(4.0);
    }
    // Lista em colunas, como no Finder.
    let footer = 250.0;
    let list_h = (ui.available_height() - footer).max(140.0);
    egui::Frame::new().fill(Color32::from_white_alpha(6)).stroke(Stroke::new(1.0, m.sep)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let w = ui.available_width();
        let (size_x, date_x) = (w - 96.0, w - 6.0);
        let (hdr, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
        let hf = theme::medium(11.0);
        ui.painter().text(egui::pos2(hdr.left() + 30.0, hdr.center().y), Align2::LEFT_CENTER, t!("Nome"), hf.clone(), m.label2);
        ui.painter().text(egui::pos2(hdr.left() + size_x, hdr.center().y), Align2::RIGHT_CENTER, t!("Tamanho"), hf.clone(), m.label2);
        ui.painter().text(egui::pos2(hdr.left() + date_x, hdr.center().y), Align2::RIGHT_CENTER, t!("Modificado"), hf, m.label2);
        ui.painter().line_segment([hdr.left_bottom(), hdr.right_bottom()], Stroke::new(1.0, m.sep));
        egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if f.loading && f.listing.is_none() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.spinner());
                return;
            }
            let Some(l) = &f.listing else { return };
            let mut shown = 0;
            for e in l.entries.iter().filter(|e| f.hidden || !e.name.starts_with('.')) {
                let sel = f.selected.contains(&e.name);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                let p = ui.painter();
                if sel {
                    p.rect_filled(rect, CornerRadius::same(6), m.accent);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(6), m.hover);
                } else if shown % 2 == 1 {
                    p.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(5));
                }
                shown += 1;
                let (text, sub) = if sel { (Color32::WHITE, Color32::from_white_alpha(210)) } else { (m.label, m.label2) };
                let ic = egui::Rect::from_center_size(egui::pos2(rect.left() + 15.0, rect.center().y), Vec2::splat(16.0));
                let folder = Color32::from_rgb(100, 180, 250);
                icons::paint(p, ic, if in_drives { Icon::Database } else if e.dir { Icon::Folder } else { Icon::File }, if sel { Color32::WHITE } else if e.dir { folder } else { m.label2 });
                let mut job = egui::text::LayoutJob::simple_singleline(e.name.clone(), egui::FontId::proportional(12.5), text);
                job.wrap = egui::text::TextWrapping { max_width: (size_x - 110.0).max(40.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                let g = ui.fonts_mut(|fo| fo.layout_job(job));
                p.galley(egui::pos2(rect.left() + 30.0, rect.center().y - g.size().y / 2.0), g, text);
                let size = if e.dir { "—".to_string() } else { human_bytes(e.size) };
                p.text(egui::pos2(rect.left() + size_x, rect.center().y), Align2::RIGHT_CENTER, size, egui::FontId::proportional(11.5), sub);
                p.text(egui::pos2(rect.left() + date_x, rect.center().y), Align2::RIGHT_CENTER, ago(e.modified), egui::FontId::proportional(11.5), sub);
                if resp.double_clicked() || (in_drives && resp.clicked()) {
                    if in_drives {
                        // Nas unidades, o nome já é o caminho completo.
                        go = Some(e.name.clone());
                    } else if e.dir {
                        go = Some(remote_join(&l.path, &e.name));
                    } else {
                        let _ = cmd.send(Cmd::Download(remote_join(&l.path, &e.name)));
                    }
                } else if resp.clicked() && !e.dir {
                    if sel {
                        f.selected.retain(|n| n != &e.name);
                    } else {
                        f.selected.push(e.name.clone());
                    }
                }
            }
            if shown == 0 && l.error.is_none() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| { ui.label(RichText::new(t!("Pasta vazia")).size(12.5).color(m.label3))});
            }
        });
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let n = f.selected.len();
        let hid = ui.selectable_label(f.hidden, RichText::new(t!("Mostrar ocultos")).size(12.0).color(m.label2)).on_hover_text(t!("Arquivos e pastas que começam com ponto"));
        if hid.clicked() {
            f.hidden = !f.hidden;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if n > 1 { t!("Baixar {}", n) } else { t!("Baixar").to_string() };
            if mac::button(ui, m, &label, Kind::Primary, None, n > 0).on_disabled_hover_text(t!("Selecione arquivos (clique). Clique duplo baixa direto.")).clicked() {
                for name in f.selected.drain(..) {
                    let _ = cmd.send(Cmd::Download(remote_join(&current, &name)));
                }
            }
            if mac::button(ui, m, t!("Enviar…"), Kind::Secondary, None, !current.is_empty() && !in_drives).on_hover_text(t!("Envia arquivos deste computador para a pasta aberta. Também dá para arrastar para a janela.")).clicked()
                && let Some(list) = rfd::FileDialog::new().pick_files()
            {
                for path in list {
                    let _ = cmd.send(Cmd::Upload(path, current.clone()));
                }
            }
        });
    });
    if let Some(path) = go {
        f.loading = true;
        let _ = cmd.send(Cmd::ListDir(path));
    }
    // Transferências, como a lista de downloads.
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(t!("Transferências")).font(theme::semibold(13.0)).color(m.label));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if mac::round_icon(ui, m, Icon::FolderOpen, &t!("Abrir a pasta de recebidos ({})", downloads.display()), 26.0, true).clicked() {
                let _ = std::fs::create_dir_all(downloads);
                open_path(downloads);
            }
        });
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical().id_salt("transferencias").auto_shrink([false, false]).show(ui, |ui| {
        if v.transfers.is_empty() {
            ui.label(RichText::new(t!("Nada ainda. O que você baixar vai para a pasta de recebidos.")).size(11.5).color(m.label3));
        }
        for t in v.transfers.iter().rev().take(20) {
            let pct = if t.total > 0 { t.done as f32 / t.total as f32 } else { 1.0 };
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::hover());
            let p = ui.painter();
            let ic = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(28.0));
            mac::icon_tile(p, ic, if t.incoming { Icon::Download } else { Icon::Upload }, if t.error.is_some() { m.red } else if t.finished { m.green } else { m.accent });
            let x = rect.left() + 38.0;
            let right_w = if t.finished && t.incoming && t.path.is_some() { 66.0 } else { 0.0 };
            let mut job = egui::text::LayoutJob::simple_singleline(t.name.clone(), egui::FontId::proportional(12.5), m.label);
            job.wrap = egui::text::TextWrapping { max_width: (rect.right() - x - right_w - 4.0).max(40.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
            let g = ui.fonts_mut(|fo| fo.layout_job(job));
            p.galley(egui::pos2(x, rect.top() + 5.0), g, m.label);
            if let Some(e) = &t.error {
                p.text(egui::pos2(x, rect.bottom() - 9.0), Align2::LEFT_CENTER, crate::i18n::tr_dyn(e), egui::FontId::proportional(11.0), m.red);
            } else if t.finished {
                p.text(egui::pos2(x, rect.bottom() - 9.0), Align2::LEFT_CENTER, format!("{} · {}", if t.incoming { t!("Recebido") } else { t!("Enviado") }, human_bytes(t.total)), egui::FontId::proportional(11.0), m.label2);
            } else {
                let bar = egui::Rect::from_min_size(egui::pos2(x, rect.bottom() - 12.0), Vec2::new((rect.right() - x - 54.0).max(20.0), 4.0));
                p.rect_filled(bar, CornerRadius::same(2), m.hover);
                p.rect_filled(egui::Rect::from_min_size(bar.min, Vec2::new(bar.width() * pct, 4.0)), CornerRadius::same(2), m.accent);
                p.text(egui::pos2(rect.right(), bar.center().y), Align2::RIGHT_CENTER, format!("{:.0}%", pct * 100.0), egui::FontId::proportional(11.0), m.label2);
            }
            if right_w > 0.0
                && let Some(path) = &t.path
            {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::right_to_left(Align::Center)));
                if mac::button(&mut child, m, t!("Abrir"), Kind::Secondary, Some(Vec2::new(60.0, 24.0)), true).clicked() {
                    open_path(path);
                }
            }
        }
    });
}

/// Conversa no estilo do Mensagens: balões azuis (meus) e cinza (do outro lado).
fn chat_panel(ui: &mut egui::Ui, m: &mac::Mac, chat: &mut Vec<ChatLine>, input: &mut String, transfers: &[elo_core::session::Transfer], mut send: impl FnMut(String)) {
    let input_h = 46.0;
    let list_h = (ui.available_height() - input_h).max(80.0);
    egui::ScrollArea::vertical().stick_to_bottom(true).max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if chat.is_empty() && transfers.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                mac::icon_tile(ui.painter(), r, Icon::Comment, m.green);
                ui.add_space(8.0);
                ui.label(RichText::new(t!("Nenhuma mensagem")).font(theme::semibold(13.0)).color(m.label));
                ui.label(RichText::new(t!("As mensagens são cifradas de ponta a ponta.")).size(11.5).color(m.label2));
            });
        }
        let max_w = (ui.available_width() * 0.8).max(120.0);
        let mut prev: Option<bool> = None;
        for l in chat.iter() {
            if let Some(p) = prev {
                ui.add_space(if p == l.mine { 3.0 } else { 10.0 });
            }
            prev = Some(l.mine);
            let (fill, color) = if l.mine { (m.accent, Color32::WHITE) } else { (m.fill, m.label) };
            let job = egui::text::LayoutJob::simple(l.text.clone(), egui::FontId::proportional(13.0), color, max_w - 24.0);
            let g = ui.fonts_mut(|f| f.layout_job(job));
            let size = g.size() + Vec2::new(24.0, 14.0);
            let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size.y), Sense::click());
            let x = if l.mine { row.right() - size.x } else { row.left() };
            let bubble = egui::Rect::from_min_size(egui::pos2(x, row.top()), size);
            ui.painter().rect_filled(bubble, CornerRadius::same((size.y / 2.0).min(17.0) as u8), fill);
            ui.painter().galley(bubble.min + Vec2::new(12.0, 7.0), g, color);
            if resp.on_hover_text(t!("Clique com o botão direito para copiar")).secondary_clicked() {
                copy(ui, &l.text);
            }
        }
        if !transfers.is_empty() {
            ui.add_space(12.0);
        }
        for t in transfers {
            let pct = if t.total > 0 { t.done as f32 / t.total as f32 } else { 1.0 };
            ui.vertical_centered(|ui| {
                let state = if t.error.is_some() { t!("falhou").into() } else if t.finished { t!("concluído").into() } else { format!("{:.0}%", pct * 100.0) };
                ui.label(RichText::new(t!("{} {} · {}", if t.incoming { t!("Recebendo") } else { t!("Enviando") }, t.name, state)).size(11.0).color(m.label2));
            });
            if !t.finished {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
                let r = r.shrink2(Vec2::new(r.width() * 0.2, 0.0));
                ui.painter().rect_filled(r, CornerRadius::same(2), m.hover);
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, Vec2::new(r.width() * pct, 4.0)), CornerRadius::same(2), m.accent);
            }
            ui.add_space(4.0);
        }
    });
    // Campo em cápsula com o botão de enviar redondo.
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
    let field = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - 40.0, rect.bottom()));
    ui.painter().rect(field, CornerRadius::same(17), if m.dark { Color32::from_white_alpha(10) } else { Color32::WHITE }, Stroke::new(1.0, if m.dark { m.sep } else { Color32::from_black_alpha(30) }), egui::StrokeKind::Inside);
    let r = ui
        .scope_builder(egui::UiBuilder::new().max_rect(field.shrink2(Vec2::new(14.0, 0.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.add(egui::TextEdit::singleline(input).frame(egui::Frame::NONE).hint_text(RichText::new(t!("Mensagem")).color(m.label3)).text_color(m.label).desired_width(f32::INFINITY).min_size(Vec2::new(0.0, 34.0)).align(Align2::LEFT_CENTER))
        })
        .inner;
    let has = !input.trim().is_empty();
    let b = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(32.0));
    let br = ui.interact(b, ui.scope_id().with("enviar"), if has { Sense::click() } else { Sense::hover() });
    ui.painter().circle_filled(b.center(), 16.0, if has { m.accent } else { m.hover });
    icons::paint(ui.painter(), b.shrink(8.0), Icon::Send, if has { Color32::WHITE } else { m.label3 });
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if (br.on_hover_text(t!("Enviar")).clicked() || enter) && has {
        let t = std::mem::take(input);
        chat.push(ChatLine { mine: true, text: t.clone() });
        send(t);
        if enter {
            r.request_focus();
        }
    }
}

/// Permissão de quem acessa: cápsula azul (liberado) ou cinza (bloqueado).
fn perm_chip(ui: &mut egui::Ui, m: &mac::Mac, icon: Icon, label: &str, on: bool, compact: bool) -> egui::Response {
    let font = theme::medium(12.0);
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), m.label);
    let w = if compact { 30.0 } else { g.size().x + 40.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let (fill, fg) = if on { (theme::alpha(m.accent, 0.14 + 0.08 * hover), m.accent) } else { (m.hover.gamma_multiply(1.0 + hover), m.label3) };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), fill);
    let ic = if compact { rect.center() } else { egui::pos2(rect.left() + 17.0, rect.center().y) };
    icons::paint(p, egui::Rect::from_center_size(ic, Vec2::splat(14.0)), icon, fg);
    if !on {
        // Risco no ícone: bloqueado.
        p.line_segment([ic + Vec2::new(-7.0, 7.0), ic + Vec2::new(7.0, -7.0)], Stroke::new(1.5, fg));
    }
    if !compact {
        p.text(egui::pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, font, fg);
    }
    let tip = if on { t!("{}: liberado. Clique para bloquear.", label) } else { t!("{}: bloqueado. Clique para liberar.", label) };
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Painel lateral da sessão (chat, arquivos): material escuro, como a tela.
fn session_panel_frame() -> egui::Frame {
    egui::Frame::new().fill(Color32::from_rgb(30, 30, 33)).stroke(Stroke::new(1.0, Color32::from_white_alpha(16))).inner_margin(Margin::same(14))
}

/// "agora", "há 5 min", "há 3 h", "ontem", "há 4 dias", "há 2 meses".
pub(crate) fn ago(when: u64) -> String {
    if when == 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match now.saturating_sub(when) {
        0..60 => t!("agora").into(),
        d @ 60..3600 => t!("há {} min", d / 60),
        d @ 3600..86_400 => t!("há {} h", d / 3600),
        86_400..172_800 => t!("ontem").into(),
        d @ 172_800..5_184_000 => t!("há {} dias", d / 86_400),
        d @ 5_184_000..63_072_000 => t!("há {} meses", d / 2_592_000),
        d => t!("há {} anos", d / 31_536_000),
    }
}

/// Botão principal largo (toda a largura), com o texto centralizado.
pub(crate) fn big_button(ui: &mut egui::Ui, p: &Palette, text: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if !enabled {
        theme::alpha(p.accent, 0.5)
    } else if resp.is_pointer_button_down_on() {
        p.accent_pressed
    } else if resp.hovered() {
        p.accent_hover
    } else {
        p.accent
    };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::semibold(14.0), p.on_accent);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Botão de ícone claro, para usar sobre o fundo azul.
fn light_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), Color32::from_white_alpha(40));
    }
    icons::paint(ui.painter(), rect.shrink(7.5), icon, Color32::WHITE);
    resp.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// O que aconteceu nos cartões de computadores.
#[derive(Default)]
struct TileOut {
    go: Option<(String, bool)>,
    wake: Option<String>,
    star: Option<String>,
    start_rename: Option<String>,
}

/// Cor do avatar por sistema.
pub(crate) fn os_color(os: &str) -> Color32 {
    match os {
        "macOS" => Color32::from_rgb(110, 110, 120),
        "Windows" => Color32::from_rgb(14, 140, 233),
        "Linux" => Color32::from_rgb(226, 150, 30),
        _ => Color32::from_rgb(120, 130, 150),
    }
}

/// Cartão de um computador: avatar, nome, ID e ações (estrela, renomear).
fn tile(ui: &mut egui::Ui, p: &Palette, id: &str, name: &str, sub_name: &str, os: &str, fav: bool, out: &mut TileOut) {
    // O fundo clicável vem primeiro; os botões por cima recebem os próprios cliques.
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(12), if hovered { p.hover } else { p.surface }, Stroke::new(1.0, if hovered { theme::alpha(p.accent, 0.55) } else { p.border }), egui::StrokeKind::Inside);
    let av = egui::pos2(rect.left() + 32.0, rect.center().y);
    painter.circle_filled(av, 19.0, os_color(os));
    icons::paint(painter, egui::Rect::from_center_size(av, Vec2::splat(20.0)), Icon::Monitor, Color32::WHITE);
    let text_left = rect.left() + 62.0;
    // Espaço para até três botões à direita (ligar, renomear, estrela).
    let text_w = (rect.right() - 108.0 - text_left).max(40.0);
    let sub = if sub_name.is_empty() { t!("{} · {}", pretty_id(id), os) } else { t!("{} · {}", sub_name, pretty_id(id)) };
    let one_line = |t: &str, font: egui::FontId, color: Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(t.to_string(), font, color);
        job.wrap = egui::text::TextWrapping { max_width: text_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let g1 = one_line(name, theme::semibold(13.5), p.text);
    let g2 = one_line(&sub, theme::mono(11.0), p.muted);
    painter.galley(egui::pos2(text_left, rect.center().y - 18.0), g1, p.text);
    painter.galley(egui::pos2(text_left, rect.center().y + 3.0), g2, p.muted);
    // Ações à direita.
    let star = egui::Rect::from_center_size(egui::pos2(rect.right() - 24.0, rect.center().y), Vec2::splat(28.0));
    let sr = ui.put(star, egui::Button::new("").frame(false)).on_hover_text(if fav { t!("Tirar dos favoritos") } else { t!("Salvar nos favoritos") });
    icons::paint(ui.painter(), star.shrink(6.0), Icon::Star, if fav { p.warning } else if sr.hovered() { p.text } else { p.muted });
    if sr.clicked() {
        out.star = Some(id.to_string());
    }
    // Ligar pelo Wake-on-LAN (aparece ao passar o mouse).
    let mut x = rect.right() - 52.0;
    if hovered || sr.hovered() {
        let wr = egui::Rect::from_center_size(egui::pos2(x, rect.center().y), Vec2::splat(28.0));
        let w = ui.put(wr, egui::Button::new("").frame(false)).on_hover_text(t!("Ligar este computador (Wake-on-LAN)"));
        icons::paint(ui.painter(), wr.shrink(7.0), Icon::Power, if w.hovered() { p.success } else { p.muted });
        if w.clicked() {
            out.wake = Some(id.to_string());
        }
        x -= 28.0;
    }
    if fav && (hovered || sr.hovered()) {
        let edit = egui::Rect::from_center_size(egui::pos2(x, rect.center().y), Vec2::splat(28.0));
        let er = ui.put(edit, egui::Button::new("").frame(false)).on_hover_text(t!("Editar nome e senha"));
        icons::paint(ui.painter(), edit.shrink(7.0), Icon::Edit, if er.hovered() { p.text } else { p.muted });
        if er.clicked() {
            out.start_rename = Some(id.to_string());
        }
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.double_clicked() {
        out.go = Some((id.to_string(), true));
    } else if resp.clicked() {
        out.go = Some((id.to_string(), false));
    }
}

/// Área do print dentro da tela (x, y, largura, altura), nunca vazia.
fn shot_crop(area: Option<[u32; 4]>, w: u32, h: u32) -> [u32; 4] {
    let [x, y, cw, ch] = area.unwrap_or([0, 0, w, h]);
    let (x, y) = (x.min(w - 1), y.min(h - 1));
    [x, y, cw.clamp(1, w - x), ch.clamp(1, h - y)]
}

/// Arquivo novo para o print: "Print - <computador> - N.png" (sem sobrescrever).
fn shot_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c.is_whitespace()).chars().take(40).collect::<String>();
    let clean = if clean.is_empty() { "Elo".to_string() } else { clean };
    (1..).map(|n| dir.join(format!("Print - {clean} - {n}.png"))).find(|p| !p.exists()).unwrap_or_else(|| dir.join("Print.png"))
}

/// Seleção da área do print sobre a tela remota: arrastar escolhe, Esc ou
/// o botão direito cancelam. Devolve o print a tirar quando o arrasto termina.
fn select_shot_area(ui: &mut egui::Ui, resp: &egui::Response, v: &mut ViewSession, rect: egui::Rect, scale: f32, (w, h): (u32, u32)) -> Option<SessAct> {
    let ctx = ui.ctx().clone();
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || resp.secondary_clicked() {
        v.shot = None;
        return None;
    }
    if resp.drag_started() {
        let origin = ctx.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        v.shot = Some(crate::app::ShotSel { start: origin.map(|p| p.clamp(rect.min, rect.max)) });
    }
    let start = v.shot.as_ref().and_then(|s| s.start);
    let now = ctx.pointer_latest_pos().map(|p| p.clamp(rect.min, rect.max));
    let sel = start.zip(now).map(|(a, b)| egui::Rect::from_two_pos(a, b));
    let painter = ui.painter_at(rect);
    let dim = Color32::from_black_alpha(120);
    match sel {
        Some(s) => {
            // Escurece em volta do que está escolhido.
            for r in [
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, s.min.y)),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, s.max.y), rect.max),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, s.min.y), egui::pos2(s.min.x, s.max.y)),
                egui::Rect::from_min_max(egui::pos2(s.max.x, s.min.y), egui::pos2(rect.max.x, s.max.y)),
            ] {
                painter.rect_filled(r, 0, dim);
            }
            painter.rect_stroke(s, 0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
            let size = format!("{} × {}", (s.width() / scale).round() as u32, (s.height() / scale).round() as u32);
            let at = egui::pos2(s.max.x, s.max.y + 6.0).min(egui::pos2(rect.max.x, rect.max.y - 22.0));
            let g = painter.layout_no_wrap(size, egui::FontId::proportional(12.0), Color32::WHITE);
            let bg = egui::Rect::from_min_size(egui::pos2(at.x - g.size().x - 12.0, at.y), g.size() + Vec2::new(12.0, 6.0));
            painter.rect_filled(bg, 6, Color32::from_black_alpha(170));
            painter.galley(bg.min + Vec2::new(6.0, 3.0), g, Color32::WHITE);
        }
        None => {
            painter.rect_filled(rect, 0, dim);
        }
    }
    if start.is_none() {
        let hint = t!("Arraste para escolher a área do print · Esc cancela");
        let g = painter.layout_no_wrap(hint.to_string(), egui::FontId::proportional(13.0), Color32::WHITE);
        let bg = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 40.0), g.size() + Vec2::new(28.0, 14.0));
        painter.rect_filled(bg, 12, Color32::from_rgba_unmultiplied(30, 33, 41, 230));
        painter.galley(bg.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    if resp.drag_stopped() {
        v.shot = None;
        let s = sel?;
        if s.width() < 4.0 || s.height() < 4.0 {
            return None;
        }
        let px = |d: f32| (d / scale).round().max(0.0) as u32;
        let (x, y) = (px(s.min.x - rect.min.x), px(s.min.y - rect.min.y));
        let (cw, ch) = (px(s.width()).max(1), px(s.height()).max(1));
        return Some(SessAct::Shot(Some(shot_crop(Some([x, y, cw, ch]), w, h))));
    }
    if start.is_some() || resp.hovered() {
        ctx.request_repaint();
    }
    None
}

/// Ação da barra da sessão que mexe no app (fora da sessão).
pub enum SessAct {
    Fullscreen,
    Favorite,
    /// Barra no alto ↔ na lateral direita (fica salvo nas configurações).
    BarSide,
    /// Print da tela remota: inteira ou a área (x, y, largura, altura em
    /// pixels de lá).
    Shot(Option<[u32; 4]>),
}

/// Cores da barra de vidro (sempre escura, sobre a tela remota, em qualquer tema).
const GLASS_TEXT: Color32 = Color32::from_rgb(240, 242, 246);
const GLASS_TEXT2: Color32 = Color32::from_rgb(196, 201, 212);
const GLASS_MUTED: Color32 = Color32::from_rgb(138, 145, 160);
const GLASS_RED: Color32 = Color32::from_rgb(235, 77, 75);
const GLASS_BLUE: Color32 = Color32::from_rgb(52, 108, 250);
const GLASS_GREEN: Color32 = Color32::from_rgb(52, 199, 89);
const GLASS_AMBER: Color32 = Color32::from_rgb(255, 176, 32);
/// Espaço reservado no alto para a barra (fora da tela cheia).
const BAR_BAND: f32 = 68.0;

/// Valor que desliza até o alvo (cúbica, desacelera no fim).
fn ease_to(ctx: &egui::Context, id: egui::Id, target: f32, dur: f64) -> f32 {
    let now = ctx.input(|i| i.time);
    let at = |(from, to, t0): (f32, f32, f64)| { from + (to - from) * egui::emath::easing::cubic_out(((now - t0) / dur).clamp(0.0, 1.0) as f32)};
    let st = match ctx.data(|d| d.get_temp::<(f32, f32, f64)>(id)) {
        Some(st) if st.1 == target => st,
        Some(st) => (at(st), target, now),
        None => (target, target, now - dur),
    };
    ctx.data_mut(|d| d.insert_temp(id, st));
    if now - st.2 < dur {
        ctx.request_repaint();
    }
    at(st)
}

/// Moldura de vidro escuro: barra e menu.
fn glass_frame(radius: u8, margin: Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(30, 33, 41, 232))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(22)))
        .corner_radius(CornerRadius::same(radius))
        .shadow(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(120) })
        .inner_margin(margin)
}

/// Reflexo no alto de uma peça de vidro.
fn glass_shine(painter: &egui::Painter, rect: egui::Rect, radius: f32) {
    painter.line_segment([rect.left_top() + Vec2::new(radius, 1.0), rect.right_top() + Vec2::new(-radius, 1.0)], Stroke::new(1.0, Color32::from_white_alpha(34)));
}

/// Botão redondo de ícone da barra.
fn glass_icon(ui: &mut egui::Ui, icon: Icon, tip: &str, active: bool, enabled: bool, tint: Option<Color32>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.12);
    let fill = if active { Color32::from_white_alpha(38) } else { Color32::from_white_alpha((22.0 * hover) as u8) };
    ui.painter().circle_filled(rect.center(), 18.0, fill);
    let color = match (enabled, tint) {
        (false, _) => Color32::from_white_alpha(56),
        (true, Some(c)) => c,
        (true, None) => GLASS_TEXT2.lerp_to_gamma(Color32::WHITE, if active { 1.0 } else { hover }),
    };
    icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0)), icon, color);
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Divisória entre grupos da barra (em pé ou deitada, conforme a barra).
fn glass_divider(ui: &mut egui::Ui, across: bool) {
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(26));
    if across {
        let (r, _) = ui.allocate_exact_size(Vec2::new(24.0, 13.0), Sense::hover());
        ui.painter().line_segment([r.left_center(), r.right_center()], stroke);
    } else {
        let (r, _) = ui.allocate_exact_size(Vec2::new(13.0, 24.0), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], stroke);
    }
}

/// Botão redondo com um número (telas do outro computador na barra em pé).
fn glass_number(ui: &mut egui::Ui, label: &str, tip: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let fill = if active { Color32::from_rgb(99, 99, 104) } else { Color32::from_white_alpha((22.0 * hover) as u8) };
    ui.painter().circle_filled(rect.center(), 15.0, fill);
    let color = if active { Color32::WHITE } else { GLASS_MUTED.lerp_to_gamma(GLASS_TEXT, hover) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(13.0), color);
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Controle segmentado com o destaque deslizando entre as opções.
/// `width`: largura total fixa (as opções dividem igualmente) ou natural.
fn glass_segments(ui: &mut egui::Ui, id: egui::Id, labels: &[String], tips: &[String], selected: usize, width: Option<f32>, h: f32) -> Option<usize> {
    let font = theme::semibold(12.5);
    let pad = 3.0;
    let natural: Vec<f32> = labels.iter().map(|l| { ui.painter().layout_no_wrap(l.clone(), font.clone(), GLASS_TEXT).size().x + 28.0}).collect();
    let ws: Vec<f32> = match width {
        Some(w) => vec![(w - 2.0 * pad) / labels.len() as f32; labels.len()],
        None => natural,
    };
    let (track, _) = ui.allocate_exact_size(Vec2::new(ws.iter().sum::<f32>() + 2.0 * pad, h), Sense::hover());
    let painter = ui.painter();
    let r = h / 2.0;
    painter.rect(track, CornerRadius::same(r as u8), Color32::from_black_alpha(80), Stroke::new(1.0, Color32::from_white_alpha(12)), egui::StrokeKind::Inside);
    let mut x = track.left() + pad;
    let rects: Vec<egui::Rect> = ws
        .iter()
        .map(|w| {
            let rr = egui::Rect::from_min_size(egui::pos2(x, track.top() + pad), Vec2::new(*w, h - 2.0 * pad));
            x += w;
            rr
        })
        .collect();
    let cur = selected.min(rects.len().saturating_sub(1));
    let px = ease_to(ui.ctx(), id.with("x"), rects[cur].left(), 0.32);
    let pw = ease_to(ui.ctx(), id.with("w"), rects[cur].width(), 0.32);
    let pill = egui::Rect::from_min_size(egui::pos2(px, rects[cur].top()), Vec2::new(pw, h - 2.0 * pad));
    let pr = CornerRadius::same((r - pad) as u8);
    painter.add(egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) }.as_shape(pill, pr));
    painter.rect_filled(pill, pr, Color32::from_rgb(99, 99, 104));
    glass_shine(painter, pill, r);
    let mut pick = None;
    for (i, rr) in rects.iter().enumerate() {
        let resp = ui.interact(*rr, id.with(i), Sense::click());
        // Texto branco onde o destaque está, mesmo no meio do deslize.
        let over = (pill.intersect(*rr).width().max(0.0) / rr.width()).clamp(0.0, 1.0);
        let base = if resp.hovered() { GLASS_TEXT } else { GLASS_MUTED };
        ui.painter().text(rr.center(), Align2::CENTER_CENTER, &labels[i], font.clone(), base.lerp_to_gamma(Color32::WHITE, over));
        let resp = match tips.get(i) {
            Some(t) if !t.is_empty() => resp.on_hover_text(t),
            _ => resp,
        };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && i != cur {
            pick = Some(i);
        }
    }
    pick
}

/// Linha de ação do menu de vidro: ícone e texto.
fn glass_row(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    if enabled && resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(10), Color32::from_white_alpha(18));
    }
    let c = if enabled { color } else { Color32::from_white_alpha(60) };
    icons::paint(painter, egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(16.0)), icon, c);
    painter.text(egui::pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), c);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Módulo da Central de Controle: cartão arredondado dentro do menu.
fn cc_module<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::from_white_alpha(12))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Botão-módulo da Central de Controle: círculo (azul quando ligado) e textos.
fn cc_toggle(ui: &mut egui::Ui, w: f32, icon: Icon, title: &str, sub: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 60.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let t = ui.ctx().animate_bool_with_time(resp.id.with("on"), on, 0.16);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(16), Color32::from_white_alpha((12.0 + 8.0 * hover) as u8));
    let c = egui::pos2(rect.left() + 26.0, rect.center().y);
    p.circle_filled(c, 17.0, Color32::from_white_alpha(30).lerp_to_gamma(GLASS_BLUE, t));
    icons::paint(p, egui::Rect::from_center_size(c, Vec2::splat(17.0)), icon, Color32::WHITE);
    let x = rect.left() + 50.0;
    let wrap = rect.right() - x - 6.0;
    let mut job = egui::text::LayoutJob::simple(title.to_string(), theme::semibold(12.5), GLASS_TEXT, wrap);
    job.wrap.max_rows = 2;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let h = g.size().y;
    p.galley(egui::pos2(x, rect.center().y - (h + 14.0) / 2.0), g, GLASS_TEXT);
    p.text(egui::pos2(x, rect.center().y + (h + 14.0) / 2.0 - 7.0), Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), GLASS_MUTED);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Controle deslizante com paradas (como o de brilho da Central de Controle).
fn cc_slider(ui: &mut egui::Ui, id: egui::Id, labels: &[&str], tips: &[&str], value: usize) -> Option<usize> {
    let n = labels.len();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click_and_drag());
    let track = egui::Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 20.0, 6.0));
    let x_of = |i: usize| track.left() + track.width() * i as f32 / (n - 1) as f32;
    let mut pick = None;
    if (resp.clicked() || resp.dragged())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let i = (((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) * (n - 1) as f32).round() as usize;
        if i != value {
            pick = Some(i);
        }
    }
    let target = pick.unwrap_or(value);
    let kx = ease_to(ui.ctx(), id, x_of(target), 0.22);
    let p = ui.painter();
    p.rect_filled(track, CornerRadius::same(3), Color32::from_white_alpha(30));
    p.rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(kx, track.bottom())), CornerRadius::same(3), GLASS_BLUE);
    for i in 0..n {
        p.circle_filled(egui::pos2(x_of(i), track.center().y), 2.0, Color32::from_white_alpha(if x_of(i) <= kx { 160 } else { 70 }));
    }
    let knob = egui::pos2(kx, track.center().y);
    p.add(egui::Shadow { offset: [0, 1], blur: 5, spread: 0, color: Color32::from_black_alpha(120) }.as_shape(egui::Rect::from_center_size(knob, Vec2::splat(20.0)), CornerRadius::same(10)));
    p.circle_filled(knob, 10.0, Color32::WHITE);
    let resp = resp.on_hover_text(tips[target]).on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = resp;
    // Nomes das paradas.
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    for (i, l) in labels.iter().enumerate() {
        let align = if i == 0 { Align2::LEFT_CENTER } else if i + 1 == n { Align2::RIGHT_CENTER } else { Align2::CENTER_CENTER };
        let x = if i == 0 { track.left() - 4.0 } else if i + 1 == n { track.right() + 4.0 } else { x_of(i) };
        let on = i == target;
        ui.painter().text(egui::pos2(x, row.center().y), align, *l, if on { theme::semibold(11.0) } else { egui::FontId::proportional(11.0) }, if on { GLASS_TEXT } else { GLASS_MUTED });
    }
    pick
}

/// Barras de sinal (0 a 3) pela fluidez da imagem.
fn signal(painter: &egui::Painter, at: egui::Pos2, level: usize, color: Color32) {
    for i in 0..3 {
        let h = 4.0 + i as f32 * 3.0;
        let r = egui::Rect::from_min_size(egui::pos2(at.x + i as f32 * 4.5, at.y - h), Vec2::new(3.0, h));
        painter.rect_filled(r, CornerRadius::same(1), if i < level { color } else { Color32::from_white_alpha(40) });
    }
}


/// Liga ou desliga "uma janela para cada tela": a tela da aba fica na aba e
/// cada uma das outras abre numa janela.
fn set_split(v: &mut ViewSession, on: bool) {
    for w in &mut v.split {
        release_split_input(&v.handle.cmd, w);
    }
    v.split = if on { v.monitors.iter().map(|m| m.index).filter(|&m| m != v.monitor).map(SplitWin::new).collect() } else { Vec::new() };
    let _ = v.handle.cmd.send(Cmd::Split(v.split.iter().map(|w| w.monitor).collect()));
}

/// Escolheu outra tela na barra. Se ela estava numa janela à parte, as duas
/// trocam de lugar: a janela passa a mostrar a que estava na aba.
fn pick_monitor(v: &mut ViewSession, m: u8) {
    let old = v.monitor;
    v.monitor = m;
    let _ = v.handle.cmd.send(Cmd::Monitor(m));
    if let Some(w) = v.split.iter_mut().find(|w| w.monitor == m) {
        release_split_input(&v.handle.cmd, w);
        w.monitor = old;
        w.tex_version = u64::MAX;
        let _ = v.handle.cmd.send(Cmd::Split(v.split.iter().map(|w| w.monitor).collect()));
    }
}

fn release_split_input(cmd: &std::sync::mpsc::Sender<Cmd>, w: &mut SplitWin) {
    let m = w.monitor;
    release_input(&mut w.input, true, &|i: Input| {
        let _ = cmd.send(Cmd::InputOn(m, i));
    });
}

/// Tamanho inicial da janela de uma tela: a proporção dela, cabendo em 1200 × 760.
fn split_window_size(w: u32, h: u32) -> Vec2 {
    if w == 0 || h == 0 {
        return Vec2::new(960.0, 600.0);
    }
    let k = (1200.0 / w as f32).min(760.0 / h as f32).min(1.0);
    Vec2::new((w as f32 * k).max(480.0), (h as f32 * k).max(300.0))
}

impl EloApp {
    /// Telas do outro computador abertas cada uma na sua janela (dá para pôr
    /// cada uma numa área de trabalho, ou em tela cheia num monitor daqui).
    /// Continuam abertas com a janela principal em outra aba ou escondida.
    fn split_windows(&mut self, ctx: &egui::Context) {
        for v in &mut self.views {
            if v.split.is_empty() || !v.connected {
                continue;
            }
            let who = match self.settings.favorite(&v.target_id) {
                Some(f) => f.label(),
                None if v.target_name.is_empty() => pretty_id(&v.target_id),
                None => v.target_name.clone(),
            };
            let key = Arc::as_ptr(&v.remote) as usize;
            let kind = (v.os == "macOS", v.fine_scroll);
            let ViewSession { split, remote, handle, permissions, monitors, .. } = v;
            let mut closed = Vec::new();
            for w in split.iter_mut() {
                let pos = monitors.iter().position(|m| m.index == w.monitor);
                let (mw, mh) = pos.map(|i| (monitors[i].width, monitors[i].height)).unwrap_or((0, 0));
                let title = t!("{} — Tela {}", who, pos.map_or(w.monitor as usize + 1, |i| i + 1));
                let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size(split_window_size(mw, mh)).with_min_inner_size([320.0, 200.0]);
                let id = egui::ViewportId::from_hash_of(("elo-tela", key, w.slot));
                let close = ctx.show_viewport_immediate(id, builder, |ui, _| {
                    split_view(ui, w, remote, &handle.cmd, permissions.control, kind, key);
                    ui.input(|i| i.viewport().close_requested())
                });
                if close {
                    release_split_input(&handle.cmd, w);
                    closed.push(w.slot);
                }
            }
            if !closed.is_empty() {
                split.retain(|w| !closed.contains(&w.slot));
                let _ = handle.cmd.send(Cmd::Split(split.iter().map(|w| w.monitor).collect()));
            }
        }
    }
}

/// Conteúdo da janela de uma tela: a imagem dela, ocupando a janela toda, e a
/// entrada (mouse e teclado) indo para ela.
fn split_view(ui: &mut egui::Ui, w: &mut SplitWin, remote: &Arc<Mutex<Remote>>, cmd: &std::sync::mpsc::Sender<Cmd>, control: bool, kind: (bool, bool), key: usize) {
    let ctx = ui.ctx().clone();
    egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(24))).show(ui, |ui| {
        let (sw, sh, version, cursor) = {
            let r = remote.lock().unwrap_or_else(|e| e.into_inner());
            r.extra.get(&w.monitor).map_or((0, 0, 0, None), |s| (s.dec.w, s.dec.h, s.version, s.cursor))
        };
        if sw == 0 || sh == 0 {
            release_split_input(cmd, w);
            ui.centered_and_justified(|ui| ui.spinner());
            return;
        }
        if w.texture.is_none() || w.tex_version != version {
            let r = remote.lock().unwrap_or_else(|e| e.into_inner());
            let Some(s) = r.extra.get(&w.monitor) else { return };
            let img = egui::ColorImage::from_rgba_unmultiplied([s.dec.w as usize, s.dec.h as usize], &s.dec.rgba);
            drop(r);
            match &mut w.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => w.texture = Some(ctx.load_texture(format!("tela-{key}-{}", w.slot), img, egui::TextureOptions::LINEAR)),
            }
            w.tex_version = version;
        }
        // Ocupa a janela (a pessoa escolhe o tamanho dela, ou põe em tela cheia).
        let avail = ui.available_rect_before_wrap();
        let scale = (avail.width() / sw as f32).min(avail.height() / sh as f32);
        let rect = egui::Rect::from_center_size(avail.center(), Vec2::new(sw as f32 * scale, sh as f32 * scale));
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());
        if let Some(t) = &w.texture {
            egui::Image::from_texture(egui::load::SizedTexture::new(t.id(), rect.size())).paint_at(ui, rect);
        }
        if !control {
            ui.painter().text(rect.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, t!("Somente visualização (o outro lado desativou o controle)"), egui::FontId::proportional(13.0), Color32::from_rgb(255, 210, 120));
        }
        let m = w.monitor;
        let send = |i: Input| {
            let _ = cmd.send(Cmd::InputOn(m, i));
        };
        let paste = |t: String| {
            let _ = cmd.send(Cmd::PasteText(t));
        };
        remote_input(&ctx, ui, &resp, rect, scale, cursor, &mut w.input, kind, &send, &paste);
    });
}

/// Cursor de lá e entrada daqui para lá, sobre a imagem `rect` de uma tela
/// (a da aba ou a de uma janela à parte). `remote`: (o outro lado é um Mac,
/// ele rola suave).
fn remote_input(ctx: &egui::Context, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, scale: f32, remote_cursor: Option<(i32, i32)>, inp: &mut RemoteInput, remote: (bool, bool), send: &dyn Fn(Input), paste: &dyn Fn(String)) {
    let (remote_mac, fine_scroll) = remote;
    // Entrada: só com o mouse sobre a tela remota (ou arrastando).
    let hovered = resp.hovered() || resp.dragged();
    // Cursor remoto: a captura não inclui o cursor. Desenha a seta onde ele
    // está, menos quando é só o nosso mouse (parado em cima ou se movendo).
    if let Some((cx, cy)) = remote_cursor {
        let pos = rect.left_top() + Vec2::new(cx as f32 * scale, cy as f32 * scale);
        let local = ctx.pointer_hover_pos().filter(|_| hovered);
        let ours = local.is_some_and(|l| { l.distance(pos) < 6.0 || inp.last_move.elapsed() < std::time::Duration::from_millis(600)});
        if rect.contains(pos) && !ours {
            paint_cursor(ui.painter(), pos);
        }
    }
    let focused = ctx.input(|i| i.focused);
    // Campo do próprio Elo em foco (chat, caminho): o teclado fica nele até
    // clicar na tela remota. O mouse continua indo para lá.
    let clicked_here = hovered && ctx.input(|i| i.pointer.any_pressed());
    let keyboard = clicked_here || ctx.memory(|m| m.focused()).is_none_or(|id| id == resp.id);
    if hovered && focused {
        if keyboard {
            resp.request_focus();
        }
        let to_remote = |pos: egui::Pos2| { (((pos.x - rect.left()) / scale).round() as i32, ((pos.y - rect.top()) / scale).round() as i32)};
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::PointerMoved(pos) => {
                    if rect.contains(pos) {
                        inp.last_move = std::time::Instant::now();
                        let (x, y) = to_remote(pos);
                        send(Input::MouseMove { x, y });
                    }
                }
                egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                    // Shift/Ctrl+clique: o modificador vai antes do clique.
                    sync_mods(&mut inp.held, remote_mods(modifiers, remote_mac), send);
                    if rect.contains(pos) || !pressed {
                        let b = match button {
                            egui::PointerButton::Primary => Some(Button::Left),
                            egui::PointerButton::Secondary => Some(Button::Right),
                            egui::PointerButton::Middle => Some(Button::Middle),
                            _ => None,
                        };
                        // Só solta lá o que foi apertado lá.
                        if let Some(b) = b.filter(|b| pressed || inp.buttons_down.contains(b)) {
                            let (x, y) = to_remote(pos);
                            send(Input::MouseMove { x, y });
                            send(Input::MouseButton { button: b, down: pressed });
                            inp.buttons_down.retain(|d| *d != b);
                            if pressed {
                                inp.buttons_down.push(b);
                            }
                        }
                    }
                }
                egui::Event::MouseWheel { delta, unit, .. } => {
                    let k = clicks_per_unit(unit, fine_scroll);
                    // Anfitrião que rola suave: em 1/120 de clique; senão, cliques inteiros.
                    if fine_scroll {
                        if let Some((dx, dy)) = wheel_steps(&mut inp.scroll_acc, delta * k * 120.0) {
                            send(Input::ScrollFine { dx, dy });
                        }
                    } else if let Some((dx, dy)) = wheel_steps(&mut inp.scroll_acc, delta * k) {
                        send(Input::Scroll { dx, dy });
                    }
                }
                egui::Event::Text(t) => {
                    if keyboard && !inp.mods.ctrl && !inp.mods.alt && !inp.mods.command {
                        send(Input::Text(t));
                    }
                }
                egui::Event::Key { key, pressed, modifiers, .. } if keyboard => {
                    // Modificadores como estavam nesta tecla (vão antes dela).
                    sync_mods(&mut inp.held, remote_mods(modifiers, remote_mac), send);
                    inp.mods = modifiers;
                    let special = match key {
                        egui::Key::Enter => Some(Key::Enter),
                        egui::Key::Tab => Some(Key::Tab),
                        egui::Key::Backspace => Some(Key::Backspace),
                        egui::Key::Delete => Some(Key::Delete),
                        egui::Key::Escape => Some(Key::Escape),
                        egui::Key::ArrowUp => Some(Key::Up),
                        egui::Key::ArrowDown => Some(Key::Down),
                        egui::Key::ArrowLeft => Some(Key::Left),
                        egui::Key::ArrowRight => Some(Key::Right),
                        egui::Key::Home => Some(Key::Home),
                        egui::Key::End => Some(Key::End),
                        egui::Key::PageUp => Some(Key::PageUp),
                        egui::Key::PageDown => Some(Key::PageDown),
                        egui::Key::Insert => Some(Key::Insert),
                        egui::Key::F1 => Some(Key::F(1)),
                        egui::Key::F2 => Some(Key::F(2)),
                        egui::Key::F3 => Some(Key::F(3)),
                        egui::Key::F4 => Some(Key::F(4)),
                        egui::Key::F5 => Some(Key::F(5)),
                        egui::Key::F6 => Some(Key::F(6)),
                        egui::Key::F7 => Some(Key::F(7)),
                        egui::Key::F8 => Some(Key::F(8)),
                        egui::Key::F9 => Some(Key::F(9)),
                        egui::Key::F10 => Some(Key::F(10)),
                        egui::Key::F11 => Some(Key::F(11)),
                        egui::Key::F12 => Some(Key::F(12)),
                        egui::Key::Space if modifiers.ctrl || modifiers.alt => Some(Key::Space),
                        k if modifiers.ctrl || modifiers.alt || modifiers.command => k.symbol_or_name().chars().next().filter(|c| c.is_ascii_alphanumeric()).map(Key::Char),
                        _ => None,
                    };
                    forward_key(&mut inp.keys_down, key, pressed, special, send);
                }
                // O egui consome Ctrl/Cmd+C/X/V: repassa como atalho (Cmd num Mac).
                egui::Event::Copy if keyboard => tap_shortcut(&mut inp.held, remote_mac, 'c', send),
                egui::Event::Cut if keyboard => tap_shortcut(&mut inp.held, remote_mac, 'x', send),
                egui::Event::Paste(t) if keyboard => {
                    // Colar de verdade: o texto vai para a área de transferência de lá
                    // (se a sincronia permitir) e o programa aberto cola sozinho. Digitar
                    // letra por letra executava comandos em terminais a cada quebra de linha.
                    paste(t);
                    tap_shortcut(&mut inp.held, remote_mac, 'v', send);
                }
                egui::Event::PasteImage(_) if keyboard => tap_shortcut(&mut inp.held, remote_mac, 'v', send),
                _ => {}
            }
        }
        if keyboard {
            // Estado real agora: soltar só o Shift (ou o Cmd) não gera evento de
            // tecla, e sem isto ele ficava apertado lá (clique virava Shift+clique).
            let now = ctx.input(|i| i.modifiers);
            sync_mods(&mut inp.held, remote_mods(now, remote_mac), send);
            inp.mods = now;
        } else {
            release_input(inp, false, send);
        }
    } else {
        // Saiu da tela ou da janela (Cmd+Tab) com algo apertado: solta tudo.
        release_input(inp, true, send);
    }
}

/// Barra da sessão: cápsula de vidro no alto (deitada) ou na lateral direita
/// (em pé, só ícones), conforme as configurações. Fora da tela cheia fica numa
/// faixa própria; na tela cheia flutua sobre a imagem e se recolhe quando o
/// mouse sai de perto.
fn session_bar(ui: &mut egui::Ui, v: &mut ViewSession, idx: usize, fav: bool, fullscreen: bool, right: bool) -> Option<SessAct> {
    let mut act = None;
    let ctx = ui.ctx().clone();
    let area = ui.max_rect();
    if !fullscreen && !right {
        ui.add_space(BAR_BAND);
    }
    let more_id = egui::Id::unique(("mais-opcoes", idx));
    let shot_id = egui::Id::unique(("print", idx));
    let menu_open = egui::Popup::is_id_open(&ctx, more_id) || egui::Popup::is_id_open(&ctx, shot_id);
    // Recolher na tela cheia.
    let seen_id = egui::Id::unique(("barra-vista", idx));
    let now = ctx.input(|i| i.time);
    let bar_rect_id = egui::Id::unique(("barra-rect", idx));
    let last_rect = ctx.data(|d| d.get_temp::<egui::Rect>(bar_rect_id));
    let near = ctx.pointer_hover_pos().is_some_and(|p| {
        let edge = if right { p.x > area.right() - 90.0 } else { p.y < area.top() + 90.0 };
        edge || last_rect.is_some_and(|r| r.expand(24.0).contains(p))
    });
    if near || menu_open || !fullscreen || ctx.data(|d| d.get_temp::<f64>(seen_id)).is_none() {
        ctx.data_mut(|d| d.insert_temp(seen_id, now));
    }
    let seen = ctx.data(|d| d.get_temp::<f64>(seen_id)).unwrap_or(now);
    let shown = !fullscreen || now - seen < 2.5;
    if fullscreen && shown {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    let t = ctx.animate_bool_with_time_and_easing(egui::Id::unique(("barra-mostra", idx)), shown, 0.28, egui::emath::easing::cubic_out);
    if t < 0.02 {
        // Alça para chamar a barra de volta.
        // (Na camada da frente: a imagem remota é desenhada depois.)
        let handle = if right {
            egui::Rect::from_center_size(egui::pos2(area.right() - 7.0, area.center().y), Vec2::new(5.0, 56.0))
        } else {
            egui::Rect::from_center_size(egui::pos2(area.center().x, area.top() + 7.0), Vec2::new(56.0, 5.0))
        };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(("barra-alca", idx))));
        painter.rect_filled(handle.expand(1.0), CornerRadius::same(4), Color32::from_black_alpha(90));
        painter.rect_filled(handle, CornerRadius::same(3), Color32::from_white_alpha(150));
        return None;
    }
    // Em pé: só ícones (e o nome some quando a janela é baixa).
    let (compact, tiny) = if right { (true, area.height() < 560.0) } else { (area.width() < 820.0, area.width() < 560.0) };
    let (pivot, pos) = if right {
        (Align2::RIGHT_CENTER, egui::pos2(area.right() - 10.0 + (1.0 - t) * 70.0, area.center().y))
    } else {
        (Align2::CENTER_TOP, egui::pos2(area.center().x, area.top() + 10.0 - (1.0 - t) * 70.0))
    };
    let layout = if right { Layout::top_down(Align::Center) } else { Layout::left_to_right(Align::Center) };
    egui::Area::new(egui::Id::unique(("barra", idx)))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain_to(area)
        .show(&ctx, |ui| {
            ui.multiply_opacity(t);
            let margin = if right { Margin::symmetric(6, 8) } else { Margin::symmetric(8, 6) };
            let r = glass_frame(26, margin)
                .show(ui, |ui| {
                    if right {
                        // Em pé, a coluna tem a largura do botão maior (senão se estica).
                        ui.set_max_width(40.0);
                    }
                    ui.with_layout(layout, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(2.0);
                        // Quem está do outro lado.
                        if !tiny {
                            let fps = v.handle.fps.load(std::sync::atomic::Ordering::Relaxed);
                            let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
                            let mins = v.started.elapsed().as_secs() / 60;
                            let line2 = if !v.connected { t!("conectando…").to_string() } else if mins == 0 { t!("{} · agora", v.os) } else { t!("{} · {} min", v.os, mins) };
                            let (level, color) = match fps {
                                _ if !v.connected => (0, GLASS_MUTED),
                                20.. => (3, GLASS_GREEN),
                                8..20 => (2, GLASS_AMBER),
                                _ => (1, GLASS_RED),
                            };
                            let data = human_bytes(v.handle.bytes_recv.load(std::sync::atomic::Ordering::Relaxed));
                            let code = if v.sas.is_empty() { String::new() } else { t!("Código de verificação: {}\nConfira com quem está do outro lado: se for igual nas duas telas, a conexão não foi interceptada.\n\n", v.sas) };
                            let info = t!("{}ID {} · {} quadros por segundo · {} recebidos", code, pretty_id(&v.target_id), fps, data);
                            let avatar = |p: &egui::Painter, av: egui::Pos2| {
                                p.circle_filled(av, 18.0, os_color(&v.os));
                                p.circle_stroke(av, 18.0, Stroke::new(1.0, Color32::from_white_alpha(40)));
                                icons::paint(p, egui::Rect::from_center_size(av, Vec2::splat(18.0)), Icon::Monitor, Color32::WHITE);
                                let dot = av + Vec2::new(13.0, 13.0);
                                p.circle_filled(dot, 5.5, Color32::from_rgb(30, 33, 41));
                                p.circle_filled(dot, 4.0, if v.connected { color } else { GLASS_AMBER });
                            };
                            if right {
                                // Em pé: só o círculo; nome e situação no balão.
                                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                                avatar(ui.painter(), rect.center());
                                resp.on_hover_text(format!("{name}\n{line2}\n\n{info}"));
                            } else {
                                let name_w = if compact { 110.0 } else { 190.0 };
                                let mut job = egui::text::LayoutJob::simple_singleline(name, theme::semibold(13.0), GLASS_TEXT);
                                job.wrap = egui::text::TextWrapping { max_width: name_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                                let g1 = ui.fonts_mut(|f| f.layout_job(job));
                                let g2 = ui.painter().layout_no_wrap(line2, egui::FontId::proportional(11.0), GLASS_MUTED);
                                let w = 44.0 + g1.size().x.max(g2.size().x + 18.0) + 10.0;
                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::hover());
                                let p = ui.painter();
                                avatar(p, egui::pos2(rect.left() + 20.0, rect.center().y));
                                let x = rect.left() + 46.0;
                                p.galley(egui::pos2(x, rect.center().y - 16.0), g1, GLASS_TEXT);
                                signal(p, egui::pos2(x, rect.center().y + 13.0), level, color);
                                p.galley(egui::pos2(x + 17.0, rect.center().y + 1.0), g2, GLASS_MUTED);
                                resp.on_hover_text(info);
                            }
                            glass_divider(ui, right);
                        }
                        // Telas do outro computador.
                        if v.monitors.len() > 1 {
                            let n = v.monitors.len();
                            let tips: Vec<String> = v.monitors.iter().map(|m| t!("{} · {}×{}{}", m.name, m.width, m.height, if m.primary { t!(" · principal") } else { "" })).collect();
                            let cur = v.monitors.iter().position(|m| m.index == v.monitor).unwrap_or(0);
                            let pick = if right {
                                (0..n).find(|&i| glass_number(ui, &(i + 1).to_string(), &tips[i], i == cur).clicked() && i != cur)
                            } else {
                                let labels: Vec<String> = (1..=n).map(|k| if compact { k.to_string() } else { t!("Tela {}", k) }).collect();
                                glass_segments(ui, egui::Id::unique(("telas", idx)), &labels, &tips, cur, None, 36.0)
                            };
                            if let Some(i) = pick {
                                pick_monitor(v, v.monitors[i].index);
                            }
                            glass_divider(ui, right);
                        }
                        // Ações.
                        let (icon, tip) = if !v.audio_available {
                            (Icon::SpeakerOff, t!("Sem som: o Elo do outro computador é de uma versão que ainda não manda som"))
                        } else if v.audio_on {
                            (Icon::Speaker, t!("Som ligado. Clique para desligar (o volume fica em Mais opções)"))
                        } else {
                            (Icon::SpeakerOff, t!("Som desligado. Clique para ouvir"))
                        };
                        if glass_icon(ui, icon, tip, false, v.audio_available, None).clicked() {
                            v.audio_on = !v.audio_on;
                            let _ = v.handle.cmd.send(Cmd::Audio(v.audio_on));
                        }
                        if glass_icon(ui, Icon::Comment, t!("Chat"), v.chat_open, true, None).clicked() {
                            v.chat_open = !v.chat_open;
                            v.files.open = false;
                        }
                        let files = glass_icon(ui, Icon::Folder, t!("Arquivos: ver as pastas do outro computador, baixar e enviar (dá para arrastar arquivos para a janela)"), v.files.open, v.permissions.files, None)
                            .on_disabled_hover_text(t!("O outro lado não liberou a transferência de arquivos"));
                        if files.clicked() {
                            v.files.open = !v.files.open;
                            v.chat_open = false;
                            if v.files.open && v.files.listing.is_none() {
                                v.files.loading = true;
                                let _ = v.handle.cmd.send(Cmd::ListDir(String::new()));
                            }
                        }
                        let shot_open = egui::Popup::is_id_open(&ctx, shot_id);
                        let cam = glass_icon(ui, Icon::Camera, t!("Print da tela"), shot_open || v.shot.is_some(), v.connected, None);
                        if let Some(a) = shot_menu(&cam, shot_id, right, v) {
                            act = Some(a);
                        }
                        if glass_icon(ui, Icon::Star, if fav { t!("Tirar dos favoritos") } else { t!("Salvar nos favoritos") }, false, true, fav.then_some(Color32::from_rgb(252, 200, 64))).clicked() {
                            act = Some(SessAct::Favorite);
                        }
                        if glass_icon(ui, if fullscreen { Icon::Shrink } else { Icon::Expand }, if fullscreen { t!("Sair da tela cheia") } else { t!("Tela cheia") }, fullscreen, true, None).clicked() {
                            act = Some(SessAct::Fullscreen);
                        }
                        let more = glass_icon(ui, Icon::More, t!("Mais opções"), egui::Popup::is_id_open(&ctx, more_id), true, None);
                        if let Some(a) = more_menu(&more, more_id, v, right) {
                            act = Some(a);
                        }
                        glass_divider(ui, right);
                        // Encerrar.
                        let label = if compact { String::new() } else { t!("Encerrar").to_string() };
                        let g = ui.painter().layout_no_wrap(label, theme::semibold(13.0), Color32::WHITE);
                        let w = if compact { 36.0 } else { g.size().x + 50.0 };
                        let (rect, end) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
                        let hover = ui.ctx().animate_bool_with_time(end.id, end.hovered(), 0.12);
                        let p = ui.painter();
                        p.add(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: theme::alpha(GLASS_RED, 0.25 + 0.25 * hover) }.as_shape(rect, CornerRadius::same(18)));
                        p.rect_filled(rect, CornerRadius::same(18), GLASS_RED.lerp_to_gamma(Color32::from_rgb(255, 104, 100), hover));
                        glass_shine(p, rect, 18.0);
                        let ic = if compact { rect.center() } else { egui::pos2(rect.left() + 21.0, rect.center().y) };
                        icons::paint(p, egui::Rect::from_center_size(ic, Vec2::splat(16.0)), Icon::Power, Color32::WHITE);
                        if !compact {
                            p.galley(egui::pos2(rect.left() + 35.0, rect.center().y - g.size().y / 2.0), g, Color32::WHITE);
                        }
                        if end.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t!("Encerrar a sessão")).clicked() {
                            v.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                })
                .response
                .rect;
            glass_shine(ui.painter(), r, 26.0);
            ctx.data_mut(|d| d.insert_temp(bar_rect_id, r));
        });
    act
}

/// Menu do print: a tela inteira na hora, ou escolher a área arrastando.
fn shot_menu(button: &egui::Response, id: egui::Id, right: bool, v: &mut ViewSession) -> Option<SessAct> {
    let mut act = None;
    egui::Popup::menu(button)
        .id(id)
        .align(if right { egui::RectAlign::LEFT } else { egui::RectAlign::BOTTOM })
        .gap(12.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(glass_frame(18, Margin::same(6)))
        .width(230.0)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if glass_row(ui, Icon::Monitor, t!("Tela inteira"), true, GLASS_TEXT).on_hover_text(t!("Salva a tela do outro computador como está agora")).clicked() {
                act = Some(SessAct::Shot(None));
                egui::Popup::close_id(ui.ctx(), id);
            }
            if glass_row(ui, Icon::Crop, t!("Selecionar área…"), true, GLASS_TEXT).on_hover_text(t!("Arraste sobre a tela para escolher o pedaço. Esc cancela.")).clicked() {
                v.shot = Some(crate::app::ShotSel::default());
                egui::Popup::close_id(ui.ctx(), id);
            }
        });
    act
}

/// Volume contínuo (0 a 100%), como o da Central de Controle.
fn cc_volume(ui: &mut egui::Ui, value: f32) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click_and_drag());
    let track = egui::Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 16.0, 6.0));
    let mut pick = None;
    if (resp.clicked() || resp.dragged())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let v = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        if (v - value).abs() > 0.001 {
            pick = Some(v);
        }
    }
    let v = pick.unwrap_or(value).clamp(0.0, 1.0);
    let kx = track.left() + track.width() * v;
    let p = ui.painter();
    p.rect_filled(track, CornerRadius::same(3), Color32::from_white_alpha(30));
    p.rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(kx, track.bottom())), CornerRadius::same(3), Color32::WHITE);
    let knob = egui::pos2(kx, track.center().y);
    p.add(egui::Shadow { offset: [0, 1], blur: 5, spread: 0, color: Color32::from_black_alpha(120) }.as_shape(egui::Rect::from_center_size(knob, Vec2::splat(18.0)), CornerRadius::same(9)));
    p.circle_filled(knob, 9.0, Color32::WHITE);
    resp.on_hover_text(t!("Volume: {}%", format!("{:.0}", v * 100.0))).on_hover_cursor(egui::CursorIcon::PointingHand);
    pick
}

/// Menu "Mais opções", como a Central de Controle: módulos de ajuste,
/// qualidade da imagem num controle deslizante e ações no outro computador.
fn more_menu(button: &egui::Response, id: egui::Id, v: &mut ViewSession, right: bool) -> Option<SessAct> {
    let mut act = None;
    egui::Popup::menu(button)
        .id(id)
        .align(if right { egui::RectAlign::LEFT_END } else { egui::RectAlign::BOTTOM_END })
        .align_alternatives(if right { &[egui::RectAlign::LEFT, egui::RectAlign::LEFT_START] } else { &[] })
        .gap(12.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(glass_frame(24, Margin::same(10)))
        .width(300.0)
        .show(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            let half = (ui.available_width() - 8.0) / 2.0;
            ui.horizontal(|ui| {
                if cc_toggle(ui, half, Icon::Expand, t!("Ajustar à janela"), if v.fit { t!("ligado") } else { t!("tamanho real") }, v.fit).on_hover_text(t!("Desligado: tamanho real, com rolagem")).clicked() {
                    v.fit = !v.fit;
                }
                if cc_toggle(ui, half, Icon::Copy, t!("Copiar e colar"), if v.clipboard { t!("ligado") } else { t!("desligado") }, v.clipboard).on_hover_text(t!("Área de transferência compartilhada entre os dois")).clicked() {
                    v.clipboard = !v.clipboard;
                    let _ = v.handle.cmd.send(Cmd::Clipboard(v.clipboard));
                }
            });
            cc_module(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Image, 14.0, GLASS_TEXT2);
                    ui.label(RichText::new(t!("Qualidade da imagem")).font(theme::semibold(12.5)).color(GLASS_TEXT));
                });
                ui.add_space(4.0);
                let tips = [t!("Menos dados por quadro: melhor em rede lenta"), t!("Bom para a maioria das conexões"), t!("Imagem mais nítida, usa mais rede"),];
                if let Some(q) = cc_slider(ui, id.with("qualidade"), &[t!("Fluida"), t!("Equilibrada"), t!("Nítida")], &tips, v.quality.min(2)) {
                    v.quality = q;
                    let (jpeg, fps) = quality(q);
                    let _ = v.handle.cmd.send(Cmd::Quality { jpeg, fps });
                }
            });
            cc_module(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Speaker, 14.0, GLASS_TEXT2);
                    ui.label(RichText::new(t!("Som do outro computador")).font(theme::semibold(12.5)).color(GLASS_TEXT));
                });
                ui.add_space(4.0);
                if !v.audio_available {
                    ui.label(RichText::new(t!("O Elo de lá ainda não manda som. Atualize-o.")).size(11.5).color(GLASS_MUTED));
                    return;
                }
                ui.horizontal(|ui| {
                    let on = v.audio_on;
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 15.0, if on { GLASS_BLUE } else { Color32::from_white_alpha(30) });
                    icons::paint(ui.painter(), r.shrink(8.0), if on { Icon::Speaker } else { Icon::SpeakerOff }, Color32::WHITE);
                    if resp.on_hover_text(if on { t!("Desligar o som") } else { t!("Ouvir o som") }).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        v.audio_on = !on;
                        let _ = v.handle.cmd.send(Cmd::Audio(v.audio_on));
                    }
                    if let Some(vol) = cc_volume(ui, v.volume) {
                        v.volume = vol;
                        let _ = v.handle.cmd.send(Cmd::Volume(vol));
                        if !v.audio_on && vol > 0.0 {
                            v.audio_on = true;
                            let _ = v.handle.cmd.send(Cmd::Audio(true));
                        }
                    }
                });
            });
            cc_module(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let ctrl = v.permissions.control;
                let denied = t!("O outro lado não liberou o controle");
                if glass_row(ui, Icon::Key, t!("Enviar Ctrl+Alt+Del"), ctrl, GLASS_TEXT).on_disabled_hover_text(denied).clicked() {
                    let _ = v.handle.cmd.send(Cmd::SecureAttention);
                    egui::Popup::close_id(ui.ctx(), id);
                }
                let (label, what) = match v.os.as_str() {
                    "Windows" => (t!("Abrir o Prompt de Comando"), t!("Abre o Prompt de Comando (cmd) na tela do outro computador")),
                    "macOS" => (t!("Abrir o Terminal"), t!("Abre o Terminal na tela do outro computador")),
                    _ => (t!("Abrir o terminal"), t!("Abre o terminal na tela do outro computador")),
                };
                let term = glass_row(ui, Icon::Terminal, label, ctrl && v.terminal, GLASS_TEXT);
                let term = if !ctrl { term.on_disabled_hover_text(denied) } else { term.on_disabled_hover_text(t!("O Elo do outro computador é de uma versão que ainda não abre o terminal. Atualize-o.")) };
                if term.on_hover_text(what).clicked() {
                    let _ = v.handle.cmd.send(Cmd::OpenTerminal);
                    egui::Popup::close_id(ui.ctx(), id);
                }
                if glass_row(ui, Icon::Refresh, t!("Reiniciar o computador…"), ctrl, Color32::from_rgb(255, 128, 124))
                    .on_hover_text(t!("Reinicia o outro computador e reconecta quando ele voltar"))
                    .on_disabled_hover_text(denied)
                    .clicked()
                {
                    v.confirm_restart = true;
                    egui::Popup::close_id(ui.ctx(), id);
                }
            });
            if v.monitors.len() > 1 {
                cc_module(ui, |ui| {
                    let on = !v.split.is_empty();
                    let label = if on { t!("Juntar as telas nesta janela") } else { t!("Uma janela para cada tela") };
                    let row = glass_row(ui, Icon::Monitor, label, v.multi || on, GLASS_TEXT)
                        .on_disabled_hover_text(t!("O Elo do outro computador é de uma versão que ainda não abre uma janela para cada tela. Atualize-o."));
                    if row.on_hover_text(t!("Cada tela do outro computador numa janela: dá para pôr cada uma numa área de trabalho ou num monitor daqui")).clicked() {
                        set_split(v, !on);
                        egui::Popup::close_id(ui.ctx(), id);
                    }
                });
            }
            cc_module(ui, |ui| {
                let label = if right { t!("Levar a barra para o alto") } else { t!("Levar a barra para a direita") };
                if glass_row(ui, Icon::Layers, label, true, GLASS_TEXT).on_hover_text(t!("Fica assim nas próximas sessões (também em Configurações)")).clicked() {
                    act = Some(SessAct::BarSide);
                    egui::Popup::close_id(ui.ctx(), id);
                }
            });
        });
    act
}

/// Modificadores que devem estar apertados lá: Ctrl, Shift, Alt, Meta.
/// O Cmd do Mac vira Ctrl num Windows/Linux (Cmd+S salva, Cmd+Z desfaz) e
/// continua Cmd num Mac.
fn remote_mods(m: egui::Modifiers, remote_mac: bool) -> [bool; 4] {
    let cmd = cfg!(target_os = "macos") && m.mac_cmd;
    [m.ctrl || (cmd && !remote_mac), m.shift, m.alt, cmd && remote_mac]
}

/// Aperta e solta `c` com o modificador de atalho de lá (e só ele). O estado
/// real dos modificadores volta no fim do quadro.
fn tap_shortcut(held: &mut [bool; 4], remote_mac: bool, c: char, send: &dyn Fn(Input)) {
    let mut only = [false; 4];
    only[if remote_mac { 3 } else { 0 }] = true;
    sync_mods(held, only, send);
    send(Input::Key { key: Key::Char(c), down: true });
    send(Input::Key { key: Key::Char(c), down: false });
}

/// Repassa uma tecla. O soltar vale pelo que foi apertado: se o Ctrl sair antes
/// da letra, ela deixa de parecer um atalho e ficaria presa lá.
fn forward_key(down: &mut Vec<(egui::Key, Key)>, key: egui::Key, pressed: bool, remote: Option<Key>, send: &dyn Fn(Input)) {
    if pressed {
        if let Some(k) = remote {
            send(Input::Key { key: k, down: true });
            if !down.iter().any(|(e, _)| *e == key) {
                down.push((key, k));
            }
        }
    } else if let Some(i) = down.iter().position(|(e, _)| *e == key) {
        let (_, k) = down.remove(i);
        send(Input::Key { key: k, down: false });
    }
}

/// Solta lá o que a janela da aba deixou apertado (e os botões, se `buttons`).
fn release_remote_input(v: &mut ViewSession, buttons: bool) {
    let cmd = v.handle.cmd.clone();
    release_input(&mut v.input, buttons, &move |i: Input| {
        let _ = cmd.send(Cmd::Input(i));
    });
}

/// Solta lá as teclas e os modificadores apertados (e os botões, se `buttons`).
fn release_input(inp: &mut RemoteInput, buttons: bool, send: &dyn Fn(Input)) {
    for (_, key) in inp.keys_down.drain(..) {
        send(Input::Key { key, down: false });
    }
    sync_mods(&mut inp.held, [false; 4], send);
    inp.mods = egui::Modifiers::NONE;
    if buttons {
        for button in inp.buttons_down.drain(..) {
            send(Input::MouseButton { button, down: false });
        }
    }
}

/// Manda só o que mudou (apertar ou soltar) e guarda o estado de lá.
fn sync_mods(held: &mut [bool; 4], want: [bool; 4], send: &dyn Fn(Input)) {
    const KEYS: [Key; 4] = [Key::Control, Key::Shift, Key::Alt, Key::Meta];
    for i in 0..4 {
        if held[i] != want[i] {
            send(Input::Key { key: KEYS[i], down: want[i] });
            held[i] = want[i];
        }
    }
}

/// Quantos cliques da roda vale uma unidade da rolagem daqui.
///
/// Touchpad (Mac, ou touchpad no Linux) chega em pontos. Para um anfitrião
/// Linux que rola suave, 15 pontos = 1 clique: é a medida do próprio libinput
/// (no Wayland, 1 clique = 15 px), então o conteúdo anda o mesmo tanto que o
/// dedo, como no touchpad de lá. Com 40, a rolagem do Mac no Ubuntu ficava
/// quase três vezes mais lenta. Anfitrião só de cliques inteiros (Windows, Mac,
/// X11 sem uinput): um clique já anda umas três linhas, então continua 40.
fn clicks_per_unit(unit: egui::MouseWheelUnit, fine_host: bool) -> f32 {
    match unit {
        egui::MouseWheelUnit::Point if fine_host => 1.0 / 15.0,
        egui::MouseWheelUnit::Point => 1.0 / 40.0,
        egui::MouseWheelUnit::Line => 1.0,
        egui::MouseWheelUnit::Page => 10.0,
    }
}

/// Junta a rolagem em "cliques" inteiros da roda. O touchpad manda muitos
/// movimentos pequenos (frações de clique): arredondar cada um sozinho perde
/// quase todos e a rolagem trava; aqui a sobra fica para o próximo.
fn wheel_steps(acc: &mut Vec2, delta: Vec2) -> Option<(i32, i32)> {
    // Mudou de sentido: a sobra do sentido anterior não vale mais.
    if acc.x * delta.x < 0.0 {
        acc.x = 0.0;
    }
    if acc.y * delta.y < 0.0 {
        acc.y = 0.0;
    }
    *acc += delta;
    let (dx, dy) = (acc.x.trunc(), acc.y.trunc());
    *acc -= Vec2::new(dx, dy);
    (dx != 0.0 || dy != 0.0).then_some((dx as i32, dy as i32))
}

/// Seta do mouse (o cursor do computador remoto, que a captura não inclui).
fn paint_cursor(painter: &egui::Painter, tip: egui::Pos2) {
    let pts: Vec<egui::Pos2> = [(0.0, 0.0), (0.0, 17.0), (4.0, 13.0), (7.0, 20.0), (9.0, 19.0), (6.0, 12.0), (12.0, 12.0)].iter().map(|&(x, y)| tip + Vec2::new(x, y)).collect();
    // Leque a partir da ponta: todos os vértices são visíveis dela.
    let mut mesh = egui::Mesh::default();
    for &q in &pts {
        mesh.colored_vertex(q, Color32::WHITE);
    }
    for i in 1..pts.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.0, Color32::BLACK)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_area_stays_inside_the_screen() {
        assert_eq!(shot_crop(None, 1920, 1080), [0, 0, 1920, 1080]);
        assert_eq!(shot_crop(Some([100, 50, 300, 200]), 1920, 1080), [100, 50, 300, 200]);
        // Passou da borda: corta no limite; vazio vira 1 pixel.
        assert_eq!(shot_crop(Some([1800, 1000, 500, 500]), 1920, 1080), [1800, 1000, 120, 80]);
        assert_eq!(shot_crop(Some([5000, 5000, 0, 0]), 1920, 1080), [1919, 1079, 1, 1]);
    }

    #[test]
    fn print_file_never_overwrites_and_has_a_safe_name() {
        let dir = std::env::temp_dir().join(format!("elo-print-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = shot_path(&dir, "../Recepção: PC/1");
        assert_eq!(a.parent(), Some(dir.as_path()));
        assert_eq!(a.file_name().unwrap().to_str().unwrap(), "Print - _Recepção_ PC_1 - 1.png");
        std::fs::write(&a, b"x").unwrap();
        assert!(shot_path(&dir, "../Recepção: PC/1").ends_with("Print - _Recepção_ PC_1 - 2.png"));
        assert!(shot_path(&dir, "...").ends_with("Print - Elo - 1.png"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn letter_is_released_even_if_ctrl_went_first() {
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |i: Input| sent.borrow_mut().push(i);
        let mut down = Vec::new();
        forward_key(&mut down, egui::Key::S, true, Some(Key::Char('s')), &send);
        // Ctrl já solto: a tecla não parece mais atalho (`None`), mas sai mesmo assim.
        forward_key(&mut down, egui::Key::S, false, None, &send);
        assert_eq!(*sent.borrow(), vec![Input::Key { key: Key::Char('s'), down: true }, Input::Key { key: Key::Char('s'), down: false }]);
        assert!(down.is_empty());
    }

    #[test]
    fn release_of_what_was_never_pressed_is_not_sent() {
        let sent = std::cell::RefCell::new(Vec::new());
        forward_key(&mut Vec::new(), egui::Key::V, false, Some(Key::Char('v')), &|i: Input| sent.borrow_mut().push(i));
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn paste_is_only_the_shortcut_with_nothing_else_held() {
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |i: Input| sent.borrow_mut().push(i);
        let mut held = [false, true, false, false];
        tap_shortcut(&mut held, false, 'v', &send);
        assert_eq!(*sent.borrow(), vec![Input::Key { key: Key::Control, down: true }, Input::Key { key: Key::Shift, down: false }, Input::Key { key: Key::Char('v'), down: true }, Input::Key { key: Key::Char('v'), down: false }]);
        assert_eq!(held, [true, false, false, false]);
    }

    #[test]
    fn released_shift_is_released_remotely() {
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |i: Input| sent.borrow_mut().push(i);
        let mut held = [false; 4];
        // Shift+A: o Shift desce antes.
        sync_mods(&mut held, remote_mods(egui::Modifiers::SHIFT, false), &send);
        // Soltou só o Shift (sem evento de tecla): o estado do quadro solta lá.
        sync_mods(&mut held, remote_mods(egui::Modifiers::NONE, false), &send);
        assert_eq!(*sent.borrow(), vec![Input::Key { key: Key::Shift, down: true }, Input::Key { key: Key::Shift, down: false }]);
        assert_eq!(held, [false; 4]);
        // Nada mudou: nada é mandado.
        sync_mods(&mut held, [false; 4], &send);
        assert_eq!(sent.borrow().len(), 2);
    }

    #[test]
    fn touchpad_scroll_is_not_lost() {
        // 40 movimentos de 0,1 clique (touchpad devagar) = 4 cliques.
        let mut acc = Vec2::ZERO;
        let total: i32 = (0..40).filter_map(|_| wheel_steps(&mut acc, Vec2::new(0.0, 0.1))).map(|(_, y)| y).sum();
        assert_eq!(total, 4);
        // Para baixo também, e virar o sentido descarta a sobra.
        let mut acc = Vec2::new(0.0, 0.9);
        assert_eq!(wheel_steps(&mut acc, Vec2::new(0.0, -0.5)), None);
        assert_eq!(wheel_steps(&mut acc, Vec2::new(0.0, -0.6)), Some((0, -1)));
        // Roda de mouse comum (um clique inteiro) continua igual.
        let mut acc = Vec2::ZERO;
        assert_eq!(wheel_steps(&mut acc, Vec2::new(0.0, 3.0)), Some((0, 3)));
    }

    #[test]
    fn touchpad_moves_like_the_host_touchpad() {
        use egui::MouseWheelUnit::*;
        // Anfitrião Linux suave: 30 pontos de touchpad = 2 cliques = 240/120,
        // que o Wayland aplica como 30 px (1 px de conteúdo por ponto).
        let mut acc = Vec2::ZERO;
        let k = clicks_per_unit(Point, true);
        let total: i32 = (0..30).filter_map(|_| wheel_steps(&mut acc, Vec2::new(0.0, 1.0) * k * 120.0)).map(|(_, y)| y).sum();
        assert!((239..=240).contains(&total), "{total}");
        // Só cliques inteiros: continua 40 pontos por clique.
        let mut acc = Vec2::ZERO;
        assert_eq!(wheel_steps(&mut acc, Vec2::new(0.0, 80.0) * clicks_per_unit(Point, false)), Some((0, 2)));
        // Roda de mouse não muda.
        assert_eq!(clicks_per_unit(Line, true), 1.0);
        assert_eq!(clicks_per_unit(Line, false), 1.0);
    }
}
