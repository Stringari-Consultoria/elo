//! Interface do Elo.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Vec2};
use elo_core::session::Cmd;
use elo_proto::ctrl::pretty_id;
use elo_proto::msg::{Button, Input, Key};
use stringari_ui::icons::{self, Icon};
use stringari_ui::theme::{self, Palette, pal};

use crate::app::{ChatLine, EloApp, Status, ViewSession, quality};

fn card(p: &Palette) -> egui::Frame {
    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(20))
}

pub(crate) fn copy(ui: &egui::Ui, t: &str) {
    ui.ctx().copy_text(t.to_string());
}

fn human_bytes(b: u64) -> String {
    let b = b as f64;
    if b > 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b > 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
    .replace('.', ",")
}

impl eframe::App for EloApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.start_minimized) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        self.pump(&ctx);
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
            egui::Panel::top("hosting").frame(egui::Frame::new().fill(p.warning_bg).stroke(Stroke::new(1.0, p.warning)).inner_margin(Margin::symmetric(14, 8))).show(ui, |ui| self.host_banner(ui));
        }
        if !fs {
            egui::Panel::bottom("status").exact_size(26.0).frame(egui::Frame::new().fill(p.chrome).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin::symmetric(12, 0))).show(ui, |ui| self.status_bar(ui));
        }
        let tab = self.active_tab;
        if tab == 0 || tab > self.views.len() {
            self.active_tab = 0;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::same(24))).show(ui, |ui| self.home(ui));
        } else {
            let idx = tab - 1;
            if self.views[idx].chat_open {
                egui::Panel::right("chat").default_size(300.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin::same(10))).show(ui, |ui| {
                    let v = &mut self.views[idx];
                    let cmd = v.handle.cmd.clone();
                    chat_panel(ui, &mut v.chat, &mut v.chat_input, &v.transfers, |t| {
                        let _ = cmd.send(Cmd::Chat(t));
                    });
                });
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(24))).show(ui, |ui| self.remote_view(ui, idx));
            match self.session_act.take() {
                Some(SessAct::Fullscreen) => {
                    let on = !self.fullscreen;
                    self.set_fullscreen(&ctx, on);
                }
                Some(SessAct::Favorite) => {
                    let v = &self.views[idx];
                    let (id, name, os) = (v.target_id.clone(), v.target_name.clone(), v.os.clone());
                    let on = self.settings.toggle_favorite(&id, &name, &os);
                    self.settings.save();
                    self.toast(if on { "Salvo nos favoritos. Dá para renomear na tela inicial." } else { "Removido dos favoritos." });
                }
                None => {}
            }
        }
        self.dialogs(&ctx);
        self.onboarding_ui(&ctx);
        self.toasts_ui(&ctx);
        // Sessões encerradas: avisa e fecha a aba.
        let mut closed = Vec::new();
        for (i, v) in self.views.iter().enumerate() {
            if let Some(r) = &v.ended {
                closed.push((i, format!("{}: {r}", if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() })));
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
            ui.label(RichText::new("Elo").font(theme::semibold(14.0)).color(p.text));
            ui.add_space(14.0);
            // Abas: início + sessões.
            let tab = |ui: &mut egui::Ui, on: bool, text: &str| -> egui::Response {
                let fill = if on { p.surface } else { Color32::TRANSPARENT };
                egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, if on { p.border_strong } else { Color32::TRANSPARENT }))
                    .corner_radius(CornerRadius::same(7))
                    .inner_margin(Margin::symmetric(12, 5))
                    .show(ui, |ui| ui.label(RichText::new(text).size(12.5).color(if on { p.text } else { p.text2 })))
                    .response
                    .interact(Sense::click())
            };
            if tab(ui, self.active_tab == 0, "Início").clicked() {
                self.active_tab = 0;
            }
            let mut close = None;
            for (i, v) in self.views.iter().enumerate() {
                let name = match self.settings.favorite(&v.target_id) {
                    Some(f) => f.label(),
                    None if v.target_name.is_empty() => pretty_id(&v.target_id),
                    None => v.target_name.clone(),
                };
                let r = tab(ui, self.active_tab == i + 1, &format!("{name}  ×"));
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
                if theme::icon_button(ui, Icon::Gear, "Configurações", false).clicked() {
                    self.show_settings = true;
                }
                let icon = if self.settings.dark { Icon::Sun } else { Icon::Moon };
                if theme::icon_button(ui, icon, "Tema claro/escuro", false).clicked() {
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
                Status::Online(_) => (p.success, "Pronto para conexões".to_string()),
                Status::Connecting => (p.warning, "Conectando ao servidor…".to_string()),
                Status::Offline(r) => (p.danger, format!("Sem conexão: {r}")),
            };
            theme::dot(ui, c, 8.0);
            ui.label(RichText::new(t).size(11.5).color(p.text2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.settings.local_server.enabled {
                    let (t, c) = match &self.local.error {
                        Some(_) => ("servidor local com erro".to_string(), p.danger),
                        None => (format!("servidor local · {}", self.local.started.public_ws().map(str::to_string).unwrap_or_else(|| if self.local.started.lan { format!("{} · TLS", self.local.lan_address()) } else { "só este computador".into() })), p.muted),
                    };
                    ui.label(RichText::new(t).font(theme::mono(11.0)).color(c)).on_hover_text(self.local.error.clone().unwrap_or_else(|| "Servidor rodando neste computador. Outros computadores conectam neste endereço (Configurações → Servidor).".into()));
                } else {
                    let a = match self.settings.server.address.trim() {
                        "" => "sem servidor configurado",
                        a => a,
                    };
                    let tls = if self.settings.server.address.trim().is_empty() || a.starts_with("wss://") { "" } else if a.starts_with("ws://") || !self.settings.server.tls { " · sem TLS" } else { " · TLS" };
                    ui.label(RichText::new(format!("{a}{tls}")).font(theme::mono(11.0)).color(p.muted));
                }
                icons::icon(ui, Icon::Lock, 11.0, p.muted).on_hover_text("Sessões cifradas ponta a ponta (SPAKE2 + ChaCha20-Poly1305). O servidor não tem a chave.");
            });
        });
    }

    fn host_banner(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let mut stop = None;
        let mut perms_change = Vec::new();
        for (i, h) in self.hosts.iter_mut().enumerate().filter(|(_, h)| h.started) {
            ui.horizontal(|ui| {
                icons::icon(ui, Icon::Eye, 15.0, p.warning);
                ui.label(RichText::new(format!("Este computador está sendo acessado por {} ({})", h.from_name, pretty_id(&h.from_id))).font(theme::semibold(12.5)).color(p.text));
                ui.label(RichText::new(format!("· código {} · {} min", h.sas, h.since.elapsed().as_secs() / 60)).size(12.0).color(p.text2)).on_hover_text("Peça para a outra pessoa confirmar este código: se for igual nos dois lados, ninguém está no meio.");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::danger_button(ui, "Encerrar").clicked() {
                        stop = Some(i);
                    }
                    let mut pm = h.permissions;
                    let c1 = ui.checkbox(&mut pm.files, "Arquivos").changed();
                    let c2 = ui.checkbox(&mut pm.clipboard, "Área de transf.").changed();
                    let c3 = ui.checkbox(&mut pm.control, "Controle").changed();
                    if c1 || c2 || c3 {
                        h.permissions = pm;
                        perms_change.push((i, pm));
                    }
                });
            });
            // Chat do anfitrião (compacto).
            let cmd = h.handle.cmd.clone();
            egui::CollapsingHeader::new(RichText::new(format!("Chat{}", if h.chat.is_empty() { String::new() } else { format!(" ({})", h.chat.len()) })).size(12.0)).id_salt(("hchat", i, h.chat.is_empty())).default_open(!h.chat.is_empty()).show(ui, |ui| {
                ui.set_max_height(220.0);
                chat_panel(ui, &mut h.chat, &mut h.chat_input, &h.transfers, |t| {
                    let _ = cmd.send(Cmd::Chat(t));
                });
                if ui.button("Enviar arquivo para quem está acessando…").clicked()
                    && let Some(f) = rfd::FileDialog::new().pick_file()
                {
                    let _ = h.handle.cmd.send(Cmd::SendFile(f));
                }
            });
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
        let w = ui.available_width().min(980.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(w);
            ui.columns(2, |cols| {
                // Este computador.
                card(&p).show(&mut cols[0], |ui| {
                    ui.set_min_height(330.0);
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("ESTE COMPUTADOR").font(theme::semibold(11.0)).color(p.muted));
                    ui.add_space(10.0);
                    ui.label(RichText::new("Seu ID").size(12.5).color(p.text2));
                    match &self.status {
                        Status::Online(id) => {
                            let id = id.clone();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(pretty_id(&id)).font(theme::mono(30.0)).color(p.text));
                                if theme::icon_button(ui, Icon::Copy, "Copiar ID", true).clicked() {
                                    copy(ui, &pretty_id(&id));
                                    self.toast("ID copiado");
                                }
                            });
                        }
                        Status::Connecting => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("obtendo ID…").size(14.0).color(p.text2));
                            });
                        }
                        Status::Offline(r) => {
                            ui.label(RichText::new("—").font(theme::mono(30.0)).color(p.muted));
                            ui.add(egui::Label::new(RichText::new(format!("Sem conexão com o servidor: {r}")).size(12.0).color(p.danger)).wrap());
                        }
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("Senha temporária").size(12.5).color(p.text2));
                    ui.horizontal(|ui| {
                        let shown = if self.show_temp { self.temp_password.clone() } else { "••••••••".into() };
                        ui.label(RichText::new(shown).font(theme::mono(20.0)).color(p.text));
                        if theme::icon_button(ui, Icon::Eye, if self.show_temp { "Ocultar" } else { "Mostrar" }, true).clicked() {
                            self.show_temp = !self.show_temp;
                        }
                        if theme::icon_button(ui, Icon::Copy, "Copiar senha", true).clicked() {
                            copy(ui, &self.temp_password);
                            self.toast("Senha copiada");
                        }
                        if theme::icon_button(ui, Icon::Refresh, "Gerar nova senha", true).clicked() {
                            self.temp_password = elo_core::temp_password();
                            self.toast("Nova senha temporária gerada (a anterior deixa de valer)");
                        }
                    });
                    ui.label(RichText::new("Passe o ID e a senha para quem vai ajudar você. Sem a senha, cada pedido precisa do seu aceite.").size(11.5).color(p.muted));
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);
                    let mut un = self.settings.unattended;
                    if theme::switch(ui, &mut un, "Acesso não assistido (senha fixa)").changed() {
                        if un && self.settings.permanent_password.len() < 8 {
                            self.show_settings = true;
                            self.toast("Defina uma senha fixa (8+ caracteres) nas configurações");
                        } else {
                            self.settings.unattended = un;
                            self.settings.save();
                        }
                    }
                    ui.label(RichText::new(if self.settings.unattended { "Com a senha fixa, a conexão entra sem precisar de alguém aqui." } else { "Desligado: só a senha temporária ou o aceite na tela." }).size(11.5).color(p.muted));
                });
                // Acessar outro.
                card(&p).show(&mut cols[1], |ui| {
                    ui.set_min_height(330.0);
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("ACESSAR OUTRO COMPUTADOR").font(theme::semibold(11.0)).color(p.muted));
                    ui.add_space(10.0);
                    theme::field_label(ui, "ID do computador");
                    let out = egui::TextEdit::singleline(&mut self.target).hint_text("123 456 789").font(theme::mono(20.0)).desired_width(f32::INFINITY).show(ui);
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
                    ui.add_space(6.0);
                    theme::field_label(ui, "Senha (opcional)");
                    let r2 = ui.add(egui::TextEdit::singleline(&mut self.password).password(true).hint_text("vazio = pedir permissão").desired_width(f32::INFINITY));
                    let enter = (r.lost_focus() || r2.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_space(10.0);
                    let busy = self.pending_connect.is_some();
                    ui.horizontal(|ui| {
                        let b = ui.add_enabled(!busy, egui::Button::new(RichText::new(if busy { "Conectando…" } else { "Conectar" }).color(p.on_accent).font(theme::semibold(13.0))).fill(p.accent).min_size(Vec2::new(140.0, 34.0)).corner_radius(CornerRadius::same(8)));
                        if (b.clicked() || enter) && !busy {
                            let (t, pw) = (self.target.clone(), self.password.clone());
                            self.connect(t, pw);
                        }
                        if busy {
                            ui.spinner();
                        }
                    });
                    ui.add_space(14.0);
                    // Favoritos e recentes: clique preenche o ID; clique duplo já conecta.
                    let mut go: Option<(String, bool)> = None;
                    let mut star: Option<(String, String, String)> = None;
                    let mut start_rename: Option<String> = None;
                    let mut renamed: Option<(String, String)> = None;
                    let mut unfav: Option<String> = None;
                    let row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
                        egui::Frame::new().fill(p.panel).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| add(ui));
                        });
                        ui.add_space(4.0);
                    };
                    let pick = |r: egui::Response, id: &str, go: &mut Option<(String, bool)>| {
                        let r = r.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.double_clicked() {
                            *go = Some((id.to_string(), true));
                        } else if r.clicked() {
                            *go = Some((id.to_string(), false));
                        }
                    };
                    if !self.settings.favorites.is_empty() {
                        ui.label(RichText::new("Favoritos").size(12.0).color(p.text2));
                        for f in &self.settings.favorites {
                            let renaming = &mut self.renaming;
                            row(ui, &mut |ui| {
                                icons::icon(ui, Icon::Star, 15.0, p.accent);
                                match renaming.as_mut().filter(|(id, _)| *id == f.id) {
                                    Some((_, text)) => {
                                        let r = ui.add(egui::TextEdit::singleline(text).hint_text(f.name.as_str()).desired_width(ui.available_width() - 40.0));
                                        r.request_focus();
                                        if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                            renamed = Some((f.id.clone(), text.trim().to_string()));
                                        }
                                    }
                                    None => {
                                        let r = ui.vertical(|ui| {
                                            ui.label(RichText::new(f.label()).size(12.5).color(p.text));
                                            let sub = if f.alias.trim().is_empty() || f.name.is_empty() { format!("{} · {}", pretty_id(&f.id), f.os) } else { format!("{} · {} · {}", f.name, pretty_id(&f.id), f.os) };
                                            ui.label(RichText::new(sub).font(theme::mono(11.0)).color(p.muted));
                                        });
                                        pick(r.response, &f.id, &mut go);
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if theme::icon_button(ui, Icon::Close, "Tirar dos favoritos", false).clicked() {
                                                unfav = Some(f.id.clone());
                                            }
                                            if theme::icon_button(ui, Icon::Edit, "Renomear", false).clicked() {
                                                start_rename = Some(f.id.clone());
                                            }
                                        });
                                    }
                                }
                            });
                        }
                        ui.add_space(6.0);
                    }
                    let recent: Vec<_> = self.settings.recent.iter().filter(|r| self.settings.favorite(&r.id).is_none()).take(6).collect();
                    if !recent.is_empty() {
                        ui.label(RichText::new("Recentes").size(12.0).color(p.text2));
                        for r in recent {
                            row(ui, &mut |ui| {
                                icons::icon(ui, Icon::Monitor, 15.0, p.text2);
                                let resp = ui.vertical(|ui| {
                                    ui.label(RichText::new(if r.name.is_empty() { pretty_id(&r.id) } else { r.name.clone() }).size(12.5).color(p.text));
                                    ui.label(RichText::new(format!("{} · {}", pretty_id(&r.id), r.os)).font(theme::mono(11.0)).color(p.muted));
                                });
                                pick(resp.response, &r.id, &mut go);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if theme::icon_button(ui, Icon::Star, "Salvar nos favoritos", false).clicked() {
                                        star = Some((r.id.clone(), r.name.clone(), r.os.clone()));
                                    }
                                });
                            });
                        }
                    }
                    if let Some(id) = start_rename {
                        let cur = self.settings.favorite(&id).map(|f| f.alias.clone()).unwrap_or_default();
                        self.renaming = Some((id, cur));
                    }
                    if let Some((id, alias)) = renamed {
                        if let Some(f) = self.settings.favorites.iter_mut().find(|f| f.id == id) {
                            f.alias = alias;
                        }
                        self.renaming = None;
                        self.settings.save();
                    }
                    if let Some(id) = unfav {
                        self.settings.favorites.retain(|f| f.id != id);
                        self.settings.save();
                    }
                    if let Some((id, name, os)) = star {
                        self.settings.toggle_favorite(&id, &name, &os);
                        self.settings.save();
                        self.renaming = Some((id, String::new()));
                    }
                    if let Some((id, now)) = go {
                        self.target = pretty_id(&id);
                        if now {
                            let pw = self.password.clone();
                            self.connect(id, pw);
                        }
                    }
                });
            });
            ui.add_space(16.0);
            ui.label(RichText::new("O conteúdo das sessões é cifrado ponta a ponta. O servidor do Elo vê só IDs, nomes dos computadores, horários e volume de dados.").size(11.5).color(p.muted));
        });
    }

    fn remote_view(&mut self, ui: &mut egui::Ui, idx: usize) {
        let p = pal(ui);
        let ctx = ui.ctx().clone();
        let fav = self.settings.favorite(&self.views[idx].target_id).is_some();
        let fs = self.fullscreen;
        let act = &mut self.session_act;
        let v = &mut self.views[idx];
        // Barra da sessão.
        egui::Frame::new().fill(p.chrome).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
            // Larga: uma linha. Estreita (chat aberto, janela pequena): controles em cima, dados embaixo.
            if ui.available_width() >= 1060.0 {
                ui.horizontal(|ui| {
                    session_info(ui, &p, v);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| *act = session_controls(ui, v, idx, fav, fs).or(act.take()));
                });
            } else {
                ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| *act = session_controls(ui, v, idx, fav, fs).or(act.take())));
                ui.horizontal(|ui| session_info(ui, &p, v));
            }
        });
        // Tela remota.
        if !v.connected {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.spinner();
                    ui.label(RichText::new(if v.waiting { "Esperando o outro lado aceitar…" } else { "Conectando…" }).size(15.0).color(Color32::from_gray(220)));
                    if v.waiting {
                        ui.label(RichText::new(format!("Código de verificação: {}", v.sas)).font(theme::mono(14.0)).color(Color32::from_gray(180)));
                    }
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
                None => v.texture = Some(ctx.load_texture(format!("remota-{idx}"), img, egui::TextureOptions::LINEAR)),
            }
            v.tex_version = version;
        }
        let avail = ui.available_rect_before_wrap();
        let scale = if v.fit { (avail.width() / w as f32).min(avail.height() / h as f32).min(1.0) } else { 1.0 / ctx.pixels_per_point() };
        let size = Vec2::new(w as f32 * scale, h as f32 * scale);
        let rect = egui::Rect::from_center_size(avail.center(), size);
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());
        if let Some(t) = &v.texture {
            ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        if !v.permissions.control {
            ui.painter().text(rect.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, "Somente visualização (o outro lado desativou o controle)", egui::FontId::proportional(13.0), Color32::from_rgb(255, 210, 120));
        }
        // Entrada: só com o mouse sobre a tela remota (ou arrastando).
        let hovered = resp.hovered() || resp.dragged();
        // Cursor remoto: a captura não inclui o cursor. Desenha a seta onde ele
        // está, menos quando é só o nosso mouse (parado em cima ou se movendo).
        let remote_cursor = v.remote.lock().unwrap_or_else(|e| e.into_inner()).cursor;
        if let Some((cx, cy)) = remote_cursor {
            let pos = rect.left_top() + Vec2::new(cx as f32 * scale, cy as f32 * scale);
            let local = ctx.pointer_hover_pos().filter(|_| hovered);
            let ours = local.is_some_and(|l| l.distance(pos) < 6.0 || v.last_move.elapsed() < std::time::Duration::from_millis(600));
            if rect.contains(pos) && !ours {
                paint_cursor(ui.painter(), pos);
            }
        }
        if hovered {
            resp.request_focus();
            let to_remote = |pos: egui::Pos2| (((pos.x - rect.left()) / scale).round() as i32, ((pos.y - rect.top()) / scale).round() as i32);
            let cmd = &v.handle.cmd;
            let send = |i: Input| {
                let _ = cmd.send(Cmd::Input(i));
            };
            let events = ctx.input(|i| i.events.clone());
            for e in events {
                match e {
                    egui::Event::PointerMoved(pos) => {
                        if rect.contains(pos) {
                            v.last_move = std::time::Instant::now();
                            let (x, y) = to_remote(pos);
                            send(Input::MouseMove { x, y });
                        }
                    }
                    egui::Event::PointerButton { pos, button, pressed, .. } => {
                        if rect.contains(pos) || !pressed {
                            let b = match button {
                                egui::PointerButton::Primary => Some(Button::Left),
                                egui::PointerButton::Secondary => Some(Button::Right),
                                egui::PointerButton::Middle => Some(Button::Middle),
                                _ => None,
                            };
                            if let Some(b) = b {
                                let (x, y) = to_remote(pos);
                                send(Input::MouseMove { x, y });
                                send(Input::MouseButton { button: b, down: pressed });
                            }
                        }
                    }
                    egui::Event::MouseWheel { delta, unit, .. } => {
                        let k = match unit {
                            egui::MouseWheelUnit::Point => 1.0 / 40.0,
                            egui::MouseWheelUnit::Line => 1.0,
                            egui::MouseWheelUnit::Page => 10.0,
                        };
                        let (dx, dy) = ((delta.x * k).round() as i32, (delta.y * k).round() as i32);
                        if dx != 0 || dy != 0 {
                            send(Input::Scroll { dx, dy });
                        }
                    }
                    egui::Event::Text(t) => {
                        if !v.mods.ctrl && !v.mods.alt && !v.mods.command {
                            send(Input::Text(t));
                        }
                    }
                    egui::Event::Key { key, pressed, modifiers, .. } => {
                        // Modificadores mudaram: replica.
                        for (on, was, k) in [(modifiers.ctrl, v.mods.ctrl, Key::Control), (modifiers.shift, v.mods.shift, Key::Shift), (modifiers.alt, v.mods.alt, Key::Alt)] {
                            if on != was {
                                send(Input::Key { key: k, down: on });
                            }
                        }
                        v.mods = modifiers;
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
                        if let Some(k) = special {
                            send(Input::Key { key: k, down: pressed });
                        }
                    }
                    egui::Event::Copy | egui::Event::Cut => {
                        // O egui consome Ctrl+C/X: repassa como atalho.
                        let c = if matches!(e, egui::Event::Copy) { 'c' } else { 'x' };
                        for (k, d) in [(Key::Control, true), (Key::Char(c), true), (Key::Char(c), false), (Key::Control, false)] {
                            send(Input::Key { key: k, down: d });
                        }
                    }
                    egui::Event::Paste(t) => send(Input::Text(t)),
                    _ => {}
                }
            }
        } else if v.mods.any() {
            // Saiu da tela com modificador apertado: solta tudo.
            for (on, k) in [(v.mods.ctrl, Key::Control), (v.mods.shift, Key::Shift), (v.mods.alt, Key::Alt)] {
                if on {
                    let _ = v.handle.cmd.send(Cmd::Input(Input::Key { key: k, down: false }));
                }
            }
            v.mods = egui::Modifiers::NONE;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        // Pedidos de acesso (sem senha).
        if let Some(a) = self.asks.first_mut() {
            let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
            let mut decision: Option<bool> = None;
            let remaining = 90u64.saturating_sub(a.at.elapsed().as_secs());
            egui::Modal::new(egui::Id::unique("ask")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    icons::icon(ui, Icon::Shield, 20.0, p.warning);
                    ui.label(RichText::new("Pedido de acesso").font(theme::semibold(17.0)).color(p.text));
                });
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(format!("{} (ID {}) quer acessar este computador.", a.from_name, pretty_id(&a.from_id))).size(13.5).color(p.text)).wrap());
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Código de verificação: {}", a.sas)).font(theme::mono(15.0)).color(p.text));
                ui.label(RichText::new("Confirme com a pessoa que o código aparece igual na tela dela. Só aceite se você conhece quem está pedindo.").size(11.5).color(p.text2));
                ui.add_space(10.0);
                ui.label(RichText::new("Permitir").size(12.0).color(p.muted));
                ui.checkbox(&mut a.permissions.control, "Controlar mouse e teclado");
                ui.checkbox(&mut a.permissions.clipboard, "Área de transferência");
                ui.checkbox(&mut a.permissions.files, "Transferir arquivos");
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::primary(ui, None, "Aceitar", None).clicked() {
                        decision = Some(true);
                    }
                    if theme::secondary(ui, None, "Recusar").clicked() {
                        decision = Some(false);
                    }
                    ui.label(RichText::new(format!("{remaining} s")).size(11.5).color(p.muted));
                });
            });
            if remaining == 0 {
                decision = Some(false);
            }
            if let Some(ok) = decision {
                let a = self.asks.remove(0);
                let _ = a.reply.send(ok.then_some(a.permissions));
                if let Some(h) = self.hosts.iter_mut().find(|h| h.from_id == a.from_id && !h.started) {
                    h.permissions = a.permissions;
                }
            }
        }
        if self.show_settings {
            self.settings_dialog(ctx);
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        let mut close = false;
        let mut test = false;
        let mut save = false;
        let mut wizard = false;
        let mut renamed = false;
        self.poll_probe();
        if self.probe.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let max_h = (ctx.content_rect().height() - 150.0).max(240.0);
        egui::Modal::new(egui::Id::unique("settings")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(RichText::new("Configurações").font(theme::semibold(17.0)).color(p.text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            theme::field_label(ui, "Nome deste computador");
            ui.add(egui::TextEdit::singleline(&mut self.settings.device_name).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.checkbox(&mut self.autostart, "Iniciar o Elo junto com o computador").on_hover_text("Abre minimizado ao entrar no sistema, para continuar acessível depois de reiniciar.");
            ui.add_space(6.0);
            theme::field_label(ui, "Senha fixa (acesso não assistido)");
            ui.add(egui::TextEdit::singleline(&mut self.new_permanent).password(true).hint_text(if self.settings.permanent_password.is_empty() { "mínimo de 8 caracteres" } else { "•••••••• (digite para trocar)" }).desired_width(f32::INFINITY));
            if !self.new_permanent.is_empty() && self.new_permanent.len() < 8 {
                ui.label(RichText::new("Use pelo menos 8 caracteres.").size(11.5).color(p.danger));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Permissões padrão para quem acessa").size(12.0).color(p.muted));
            ui.checkbox(&mut self.settings.default_permissions.control, "Controlar mouse e teclado");
            ui.checkbox(&mut self.settings.default_permissions.clipboard, "Área de transferência");
            ui.checkbox(&mut self.settings.default_permissions.files, "Transferir arquivos");
            ui.add_space(6.0);
            theme::field_label(ui, "Arquivos recebidos em");
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.settings.downloads.display().to_string()).font(theme::mono(11.5)).color(p.text2));
                if ui.small_button("Escolher…").clicked()
                    && let Some(d) = rfd::FileDialog::new().pick_folder()
                {
                    self.settings.downloads = d;
                }
            });
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("Servidor").font(theme::semibold(13.0)).color(p.text));
            ui.checkbox(&mut self.settings.local_server.enabled, "Rodar o servidor neste computador");
            if self.settings.local_server.enabled {
                let probing = self.probe.is_some();
                let probe_result = self.probe_result.clone();
                let ls = &mut self.settings.local_server;
                ui.horizontal(|ui| {
                    theme::field_label(ui, "Porta");
                    ui.add(egui::DragValue::new(&mut ls.port).range(1024..=65535).speed(0.0));
                });
                ui.checkbox(&mut ls.lan, "Aceitar conexões diretas da rede").on_hover_text("Desligado, só aceita conexões deste computador (ex.: cloudflared instalado aqui), sem deixar a porta aberta na rede. Com o cloudflared no Docker, deixe ligado.");
                theme::field_label(ui, "Endereço público (opcional)");
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 70.0;
                    ui.add(egui::TextEdit::singleline(&mut ls.public).font(theme::mono(12.0)).hint_text("wss://elo.seudominio.com (Cloudflare Tunnel)").desired_width(w));
                    let can = ls.public_ws().is_some() && !probing;
                    if ui.add_enabled(can, egui::Button::new(if probing { "Testando…" } else { "Testar" })).on_hover_text("Conecta pelo endereço público e confere se ele chega a este servidor.").clicked() {
                        test = true;
                    }
                });
                if !ls.public.trim().is_empty() && ls.public_ws().is_none() {
                    ui.label(RichText::new("Comece com wss:// (o túnel do Cloudflare fala WebSocket).").size(11.5).color(p.danger));
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
                        ui.label(RichText::new(format!("Não foi possível iniciar: {e}")).size(11.5).color(p.danger));
                    }
                    ui.label(RichText::new("Nos outros computadores, desligue \"Rodar o servidor\" e use:").size(11.5).color(p.muted));
                    let mut rows: Vec<(&str, String)> = Vec::new();
                    if let Some(w) = ls.public_ws() {
                        rows.push(("pela internet (sem impressão digital)", w.to_string()));
                    }
                    if ls.lan {
                        rows.push(("na rede local (TLS ligado)", self.local.lan_address()));
                        if !self.local.fingerprint.is_empty() {
                            rows.push(("impressão digital (só na rede local)", self.local.fingerprint.clone()));
                        }
                    }
                    if rows.is_empty() {
                        ui.label(RichText::new("Só este computador consegue conectar. Preencha o endereço público do túnel ou ligue as conexões da rede.").size(11.5).color(p.warning));
                    }
                    for (what, v) in rows {
                        ui.label(RichText::new(what).size(11.0).color(p.muted));
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&v).font(theme::mono(11.0)).color(p.text2)).wrap());
                            if ui.small_button("Copiar").clicked() {
                                copy(ui, &v);
                            }
                        });
                    }
                } else {
                    ui.label(RichText::new("O servidor reinicia ao salvar (sessões em andamento caem).").size(11.5).color(p.muted));
                }
            } else {
                theme::field_label(ui, "Endereço");
                ui.add(egui::TextEdit::singleline(&mut self.settings.server.address).font(theme::mono(12.0)).hint_text("host:porta ou wss://dominio (Cloudflare Tunnel)").desired_width(f32::INFINITY));
                let ws = self.settings.server.address.trim().starts_with("wss://") || self.settings.server.address.trim().starts_with("ws://");
                if ws {
                    ui.label(RichText::new("WebSocket: o TLS é o do endereço (wss://). Deixe a impressão digital vazia para Cloudflare.").size(11.5).color(p.muted));
                } else {
                    ui.checkbox(&mut self.settings.server.tls, "TLS (recomendado)");
                }
                theme::field_label(ui, "Impressão digital do certificado (servidor próprio autoassinado)");
                ui.add(egui::TextEdit::singleline(&mut self.settings.server.pin).font(theme::mono(11.0)).hint_text("vazio = certificado público (Let's Encrypt, Cloudflare etc.)").desired_width(f32::INFINITY));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("O servidor registra só metadados (IDs, nomes dos computadores, horários e volume). Telas, teclas, arquivos e chat são cifrados ponta a ponta com uma chave que só os dois computadores têm.").size(11.5).color(p.muted));
            });
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary(ui, None, "Salvar", None).clicked() {
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
                if theme::secondary(ui, None, "Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.settings = crate::settings::Settings::load();
                    self.autostart = crate::autostart::enabled();
                    self.new_permanent.clear();
                    close = true;
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if theme::ghost(ui, None, "Assistente inicial").on_hover_text("Abre de novo as boas-vindas: nome, servidor e início automático.").clicked() {
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
                self.error(format!("Iniciar com o computador: {e}"));
            }
            self.autostart = crate::autostart::enabled();
            // O nome só vai ao servidor ao conectar.
            if renamed {
                self.restart_control();
            }
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
    egui::CollapsingHeader::new(RichText::new("Como publicar pela internet (Cloudflare Tunnel)").size(12.5).color(p.text2)).id_salt("cloudflare").show(ui, |ui| {
        let text = |ui: &mut egui::Ui, t: &str| {
            ui.add(egui::Label::new(RichText::new(t).size(11.5).color(p.text2)).wrap());
        };
        let value = |ui: &mut egui::Ui, label: &str, v: &str| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.5).color(p.muted));
                ui.label(RichText::new(v).font(theme::mono(11.5)).color(p.text));
                if ui.small_button("Copiar").clicked() {
                    copy(ui, v);
                }
            });
        };
        text(ui, "1. No painel da Cloudflare (Zero Trust → Networks → Tunnels), abra o túnel e adicione uma rota do tipo Published application.");
        text(ui, "2. Subdomínio e domínio à sua escolha (ex.: elo.seudominio.com). Path: vazio.");
        text(ui, "3. Service, se o cloudflared roda direto neste computador:");
        value(ui, "   Type HTTP · URL", &format!("localhost:{port}"));
        text(ui, "   Se o cloudflared roda no Docker, \"localhost\" é o contêiner. Use Type HTTPS com o IP do gateway da rede do contêiner e, em Additional application settings → TLS, ligue No TLS Verify. Deixe \"Aceitar conexões diretas da rede\" ligado.");
        let gws = docker_gateways();
        if (1..=3).contains(&gws.len()) {
            for g in &gws {
                value(ui, "   Type HTTPS · URL", &format!("{g}:{port}"));
            }
            text(ui, "   (gateways das redes Docker deste computador: use o da rede do contêiner do cloudflared)");
        } else {
            if !gws.is_empty() {
                text(ui, &format!("   Gateways das redes Docker deste computador: {}.", gws.join(", ")));
            }
            text(ui, &format!("   Para saber o da rede do cloudflared: docker inspect <contêiner> e procure \"Gateway\". A URL fica <gateway>:{port}."));
        }
        text(ui, "4. Preencha o Endereço público acima com wss:// e o seu endereço, e clique em Testar.");
        text(ui, "5. Nos outros computadores: desligue \"Rodar o servidor neste computador\" e use o mesmo wss://, com a impressão digital vazia.");
    });
}

/// Gateways das redes Docker (Linux): o computador visto de dentro dos
/// contêineres. Lê as rotas das interfaces docker0/br-*; o gateway é o
/// primeiro endereço da rede.
fn docker_gateways() -> Vec<String> {
    let Ok(t) = std::fs::read_to_string("/proc/net/route") else { return Vec::new() };
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

fn chat_panel(ui: &mut egui::Ui, chat: &mut Vec<ChatLine>, input: &mut String, transfers: &[elo_core::session::Transfer], mut send: impl FnMut(String)) {
    let p = pal(ui);
    ui.label(RichText::new("Chat").font(theme::semibold(13.0)).color(p.text));
    egui::ScrollArea::vertical().stick_to_bottom(true).max_height(ui.available_height() - 120.0).auto_shrink([false, true]).show(ui, |ui| {
        for l in chat.iter() {
            ui.with_layout(Layout::top_down(if l.mine { Align::Max } else { Align::Min }), |ui| {
                egui::Frame::new().fill(if l.mine { p.selected } else { p.surface }).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(8, 5)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&l.text).size(12.5).color(p.text)).wrap());
                });
            });
            ui.add_space(3.0);
        }
        for t in transfers {
            let pct = if t.total > 0 { t.done as f32 / t.total as f32 } else { 1.0 };
            ui.label(RichText::new(format!("{} {} — {}", if t.incoming { "↓" } else { "↑" }, t.name, if t.finished { "concluído".into() } else { format!("{:.0}%", pct * 100.0) })).size(11.5).color(p.text2));
            if !t.finished {
                ui.add(egui::ProgressBar::new(pct).desired_height(4.0));
            }
        }
    });
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(input).hint_text("mensagem").desired_width(ui.available_width() - 70.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Enviar").clicked() || enter) && !input.trim().is_empty() {
            let t = std::mem::take(input);
            chat.push(ChatLine { mine: true, text: t.clone() });
            send(t);
        }
    });
}

fn session_info(ui: &mut egui::Ui, p: &Palette, v: &ViewSession) {
    theme::dot(ui, if v.connected { p.success } else { p.warning }, 8.0);
    ui.label(RichText::new(format!("{} · {}", if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() }, v.os)).size(12.5).color(p.text));
    if !v.sas.is_empty() {
        ui.label(RichText::new(format!("código {}", v.sas)).font(theme::mono(11.5)).color(p.text2)).on_hover_text("Confira com quem está do outro lado: se o código for igual nas duas telas, a conexão não foi interceptada.");
    }
    let fps = v.handle.fps.load(std::sync::atomic::Ordering::Relaxed);
    ui.label(RichText::new(format!("{} min · {fps} qps · {}", v.started.elapsed().as_secs() / 60, human_bytes(v.handle.bytes_recv.load(std::sync::atomic::Ordering::Relaxed)))).size(11.5).color(p.muted)).on_hover_text("Duração · quadros por segundo · dados recebidos");
}

/// Ação da barra da sessão que mexe no app (fora da sessão).
pub enum SessAct {
    Fullscreen,
    Favorite,
}

fn session_controls(ui: &mut egui::Ui, v: &mut ViewSession, idx: usize, fav: bool, fullscreen: bool) -> Option<SessAct> {
    let mut act = None;
    if theme::danger_button(ui, "Encerrar").clicked() {
        v.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if ui.selectable_label(fullscreen, if fullscreen { "Sair da tela cheia" } else { "Tela cheia" }).clicked() {
        act = Some(SessAct::Fullscreen);
    }
    if theme::icon_button(ui, Icon::Star, if fav { "Tirar dos favoritos" } else { "Salvar nos favoritos" }, fav).clicked() {
        act = Some(SessAct::Favorite);
    }
    if ui.selectable_label(v.chat_open, "Chat").clicked() {
        v.chat_open = !v.chat_open;
    }
    if ui.add_enabled(v.permissions.files, egui::Button::new("Enviar arquivo")).clicked()
        && let Some(f) = rfd::FileDialog::new().pick_file()
    {
        let _ = v.handle.cmd.send(Cmd::SendFile(f));
        v.chat_open = true;
    }
    if ui.add_enabled(v.permissions.control, egui::Button::new("Ctrl+Alt+Del")).clicked() {
        let _ = v.handle.cmd.send(Cmd::SecureAttention);
    }
    let mut cb = v.clipboard;
    if ui.checkbox(&mut cb, "Copiar/colar").changed() {
        v.clipboard = cb;
        let _ = v.handle.cmd.send(Cmd::Clipboard(cb));
    }
    let labels = ["Velocidade", "Equilibrado", "Qualidade"];
    let mut q = v.quality;
    egui::ComboBox::from_id_salt(("q", idx)).selected_text(labels[q.min(2)]).width(104.0).show_ui(ui, |ui| {
        for (k, l) in labels.iter().enumerate() {
            ui.selectable_value(&mut q, k, *l);
        }
    });
    if q != v.quality {
        v.quality = q;
        let (jpeg, fps) = quality(q);
        let _ = v.handle.cmd.send(Cmd::Quality { jpeg, fps });
    }
    if v.monitors.len() > 1 {
        let cur = v.monitors.get(v.monitor as usize).map(|m| m.name.clone()).unwrap_or_default();
        let mut pick = None;
        egui::ComboBox::from_id_salt(("mon", idx)).selected_text(cur).width(120.0).show_ui(ui, |ui| {
            for m in &v.monitors {
                if ui.selectable_label(m.index == v.monitor, format!("{} ({}×{})", m.name, m.width, m.height)).clicked() {
                    pick = Some(m.index);
                }
            }
        });
        if let Some(m) = pick {
            v.monitor = m;
            let _ = v.handle.cmd.send(Cmd::Monitor(m));
        }
    }
    ui.checkbox(&mut v.fit, "Ajustar").on_hover_text("Ajustar a tela remota à janela (desligado = tamanho real)");
    act
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
