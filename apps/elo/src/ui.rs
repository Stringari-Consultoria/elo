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
            if self.views[idx].files.open {
                let downloads = self.settings.downloads.clone();
                egui::Panel::right("arquivos").default_size(440.0).min_size(340.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin::same(14))).show(ui, |ui| {
                    files_panel(ui, &mut self.views[idx], &downloads);
                });
            } else if self.views[idx].chat_open {
                egui::Panel::right("chat").default_size(300.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.border)).inner_margin(Margin::same(10))).show(ui, |ui| {
                    let v = &mut self.views[idx];
                    let cmd = v.handle.cmd.clone();
                    chat_panel(ui, &mut v.chat, &mut v.chat_input, &v.transfers, |t| {
                        let _ = cmd.send(Cmd::Chat(t));
                    });
                });
            }
            let central = egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(24))).show(ui, |ui| self.remote_view(ui, idx)).response.rect;
            self.drop_files(&ctx, idx, central);
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
                let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
                if r == elo_proto::msg::BYE_RESTART {
                    // Reconecta quando voltar (o Elo de lá precisa iniciar com o computador).
                    let now = std::time::Instant::now();
                    self.reconnects.retain(|x| x.id != v.target_id);
                    self.reconnects.push(crate::app::Reconnect { id: v.target_id.clone(), name: name.clone(), password: v.password.clone(), next_try: now + std::time::Duration::from_secs(20), until: now + std::time::Duration::from_secs(360) });
                    closed.push((i, format!("{name} está reiniciando. O Elo reconecta sozinho quando ele voltar.")));
                } else {
                    closed.push((i, format!("{name}: {r}")));
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
                            ui.label(RichText::new(format!("{} está reiniciando. Reconectando quando voltar… ({}:{:02})", r.name, left / 60, left % 60)).size(13.0).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::secondary(ui, None, "Cancelar").clicked() {
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
                        ui.label(RichText::new("Acesso remoto").font(theme::semibold(22.0)).color(p.text));
                        ui.label(RichText::new("Passe seu ID para receber ajuda, ou digite o ID de outro computador para acessá-lo.").size(13.0).color(p.text2));
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
                    let text = "O conteúdo das sessões é cifrado ponta a ponta. O servidor vê só IDs, nomes dos computadores, horários e volume de dados.";
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
            State::Available { manifest, asset: Some(_) } if !self.settings.auto_update => (format!("Nova versão do Elo disponível: {}.", manifest.version), Some("Atualizar")),
            State::Available { manifest, asset: None } => (format!("Nova versão do Elo disponível: {}.", manifest.version), Some("Baixar no GitHub")),
            State::Downloading { version, done, total } => {
                let pct = done.load(std::sync::atomic::Ordering::Relaxed) as f32 / (*total).max(1) as f32 * 100.0;
                (format!("Baixando a versão {version}… {pct:.0}%"), None)
            }
            State::Ready { version, .. } => (
                format!("A versão {version} está pronta.{}", if self.views.is_empty() && self.hosts.is_empty() { "" } else { " Instala ao fim das sessões, ou agora (as sessões caem)." }),
                Some("Reiniciar e atualizar"),
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
                                State::Available { asset: None, .. } => open_url(crate::update::RELEASES),
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
                ui.label(RichText::new("SEU ID").font(theme::semibold(11.0)).extra_letter_spacing(0.8).color(soft));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (t, c) = match &self.status {
                        Status::Online(_) => ("Online", Color32::from_rgb(120, 240, 170)),
                        Status::Connecting => ("Conectando", Color32::from_rgb(255, 214, 120)),
                        Status::Offline(_) => ("Sem conexão", Color32::from_rgb(255, 160, 160)),
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
                        if light_button(ui, Icon::Copy, "Copiar ID").clicked() {
                            copy(ui, &pretty_id(&id));
                            self.toast("ID copiado");
                        }
                    });
                }
                Status::Connecting => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(white).size(22.0));
                        ui.label(RichText::new("obtendo ID…").size(16.0).color(soft));
                    });
                    ui.add_space(14.0);
                }
                Status::Offline(r) => {
                    ui.label(RichText::new("— — —").font(theme::mono(40.0)).color(Color32::from_white_alpha(120)));
                    ui.add(egui::Label::new(RichText::new(format!("Sem conexão com o servidor: {r}")).size(12.0).color(white)).wrap());
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new("SENHA TEMPORÁRIA").font(theme::semibold(11.0)).extra_letter_spacing(0.8).color(soft));
            ui.add_space(4.0);
            egui::Frame::new().fill(Color32::from_black_alpha(46)).corner_radius(CornerRadius::same(10)).inner_margin(Margin { left: 14, right: 6, top: 4, bottom: 4 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let shown = if self.show_temp { self.temp_password.clone() } else { "••••••••".into() };
                    ui.label(RichText::new(shown).font(theme::mono(19.0)).color(white));
                    ui.add_space(10.0);
                    if light_button(ui, Icon::Eye, if self.show_temp { "Ocultar" } else { "Mostrar" }).clicked() {
                        self.show_temp = !self.show_temp;
                    }
                    if light_button(ui, Icon::Copy, "Copiar senha").clicked() {
                        copy(ui, &self.temp_password);
                        self.toast("Senha copiada");
                    }
                    if light_button(ui, Icon::Refresh, "Gerar nova senha").clicked() {
                        self.temp_password = elo_core::temp_password();
                        self.toast("Nova senha temporária gerada (a anterior deixa de valer)");
                    }
                });
            });
            ui.add_space(10.0);
            ui.add(egui::Label::new(RichText::new("Passe o ID e a senha para quem vai ajudar. Sem a senha, cada pedido precisa do seu aceite.").size(12.0).color(soft)).wrap());
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
                    ui.label(RichText::new("Acesso não assistido").font(theme::semibold(13.5)).color(p.text));
                    ui.label(RichText::new(if self.settings.unattended { "Ligado: a senha fixa entra sem ninguém aqui." } else { "Entrar com uma senha fixa, sem ninguém aqui." }).size(12.0).color(p.text2));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::switch(ui, &mut un, "").changed() {
                        if un && self.settings.permanent_password.len() < 8 {
                            self.show_settings = true;
                            self.toast("Defina uma senha fixa (8+ caracteres) nas configurações");
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
            ui.label(RichText::new("Acessar outro computador").font(theme::semibold(16.0)).color(p.text));
            ui.label(RichText::new("Digite o ID que a outra pessoa passou.").size(12.5).color(p.text2));
            ui.add_space(16.0);
            theme::field_label(ui, "ID do computador");
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
            theme::field_label(ui, "Senha (opcional)");
            let r2 = ui.add(egui::TextEdit::singleline(&mut self.password).password(true).hint_text("vazio = pedir permissão na tela").font(egui::FontId::proportional(14.5)).margin(Margin::symmetric(16, 6)).min_size(Vec2::new(0.0, 44.0)).align(Align2::LEFT_CENTER).desired_width(f32::INFINITY));
            let enter = (r.lost_focus() || r2.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(16.0);
            let busy = self.pending_connect.is_some();
            let b = big_button(ui, p, if busy { "Conectando…" } else { "Conectar" }, !busy);
            if (b.clicked() || enter) && !busy {
                let (t, pw) = (self.target.clone(), self.password.clone());
                self.connect(t, pw);
            }
        });
    }

    /// Favoritos e recentes em cartões. Clique preenche o ID; clique duplo conecta.
    fn computers(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let favs: Vec<(String, String, String, String, bool)> = self.settings.favorites.iter().map(|f| (f.id.clone(), f.label(), if f.alias.trim().is_empty() || f.name.is_empty() { String::new() } else { f.name.clone() }, f.os.clone(), true)).collect();
        let recent: Vec<(String, String, String, String, bool)> = self
            .settings
            .recent
            .iter()
            .filter(|r| self.settings.favorite(&r.id).is_none())
            .take(6)
            .map(|r| (r.id.clone(), if r.name.is_empty() { pretty_id(&r.id) } else { r.name.clone() }, String::new(), r.os.clone(), false))
            .collect();
        if favs.is_empty() && recent.is_empty() {
            return;
        }
        let mut out = TileOut::default();
        let cols = if ui.available_width() > 780.0 { 3 } else { 2 };
        for (title, list) in [("Favoritos", favs), ("Recentes", recent)] {
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
                        let renaming = self.renaming.as_mut().filter(|(rid, _)| rid == id).map(|(_, t)| t);
                        tile(&mut c[i], p, id, name, sub_name, os, *fav, renaming, &mut out);
                    }
                });
                ui.add_space(10.0);
            }
            ui.add_space(8.0);
        }
        if let Some(id) = out.start_rename {
            let cur = self.settings.favorite(&id).map(|f| f.alias.clone()).unwrap_or_default();
            self.renaming = Some((id, cur));
        }
        if let Some((id, alias)) = out.renamed {
            if let Some(f) = self.settings.favorites.iter_mut().find(|f| f.id == id) {
                f.alias = alias;
            }
            self.renaming = None;
            self.settings.save();
        }
        if let Some(id) = out.star {
            if self.settings.favorite(&id).is_some() {
                self.settings.favorites.retain(|f| f.id != id);
            } else if let Some(r) = self.settings.recent.iter().find(|r| r.id == id).cloned() {
                self.settings.toggle_favorite(&r.id, &r.name, &r.os);
                self.renaming = Some((id, String::new()));
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
        if v.confirm_restart {
            let mut close = false;
            let name = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
            egui::Modal::new(egui::Id::unique("reiniciar")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22))).show(&ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new(format!("Reiniciar {name}?")).font(theme::semibold(16.0)).color(p.text));
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new("A sessão cai e o Elo tenta reconectar sozinho por até 6 minutos. Para voltar sem ninguém lá, o outro computador precisa iniciar o Elo com o computador e ter o acesso não assistido ligado.").size(12.5).color(p.text2)).wrap());
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::danger_button(ui, "Reiniciar").clicked() {
                        let _ = v.handle.cmd.send(Cmd::Restart);
                        close = true;
                    }
                    if theme::secondary(ui, None, "Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
        if self.show_settings && self.panel.is_none() {
            self.settings_dialog(ctx);
        }
        if self.panel.is_some() {
            self.server_panel(ctx);
        }
    }

    /// Painel do servidor embutido: online, sessões em andamento e histórico.
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
                0..60 => "agora".into(),
                60..3600 => format!("há {} min", d / 60),
                3600..86_400 => format!("há {} h", d / 3600),
                _ => format!("há {} dias", d / 86_400),
            }
        };
        let dur = |s: u64| -> String { if s < 60 { format!("{s} s") } else if s < 3600 { format!("{} min", s / 60) } else { format!("{} h {:02} min", s / 3600, (s % 3600) / 60) } };
        let who = |name: &str, id: &str| if name.is_empty() { pretty_id(id) } else { format!("{name} ({})", pretty_id(id)) };
        let max_h = (ctx.content_rect().height() - 160.0).max(240.0);
        egui::Modal::new(egui::Id::unique("painel")).frame(egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(22))).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.horizontal(|ui| {
                icons::icon(ui, Icon::Globe, 18.0, p.accent);
                ui.label(RichText::new("Painel do servidor").font(theme::semibold(17.0)).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(ui, Icon::Close, "Fechar", false).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (n, label) in [(st.online.len(), "online agora"), (st.active.len(), "sessões em andamento"), (st.recent.len(), "no histórico recente")] {
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
                ui.label(RichText::new("Online agora").font(theme::semibold(13.5)).color(p.text));
                egui::Grid::new("online").num_columns(4).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                    for h in ["Computador", "Sistema", "IP", "Conectado"] {
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
                ui.label(RichText::new("Sessões em andamento").font(theme::semibold(13.5)).color(p.text));
                if st.active.is_empty() {
                    ui.label(RichText::new("Nenhuma agora.").size(12.0).color(p.muted));
                }
                for x in &st.active {
                    ui.label(RichText::new(format!("{} acessando {} · começou {}", who(&x.viewer_name, &x.viewer_id), who(&x.host_name, &x.host_id), ago(x.start))).size(12.5).color(p.text));
                }
                ui.add_space(14.0);
                ui.label(RichText::new("Histórico").font(theme::semibold(13.5)).color(p.text));
                if st.recent.is_empty() {
                    ui.label(RichText::new("Nenhuma sessão registrada.").size(12.0).color(p.muted));
                }
                egui::Grid::new("historico").num_columns(4).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                    for h in ["Quem acessou", "Computador acessado", "Quando", "Duração · volume"] {
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
            ui.label(RichText::new(if st.retention_days == 0 { "O histórico é guardado para sempre. Defina um prazo nas configurações (LGPD).".to_string() } else { format!("O histórico é apagado depois de {} dias. Nomes aparecem para quem se conectou desde que o servidor abriu.", st.retention_days) }).size(11.5).color(p.muted));
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
            ui.label(RichText::new("Configurações").font(theme::semibold(17.0)).color(p.text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            theme::field_label(ui, "Nome deste computador");
            ui.add(egui::TextEdit::singleline(&mut self.settings.device_name).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            ui.checkbox(&mut self.autostart, "Iniciar o Elo junto com o computador").on_hover_text("Abre na bandeja ao entrar no sistema, para continuar acessível depois de reiniciar.");
            ui.add_enabled_ui(self.tray.is_some(), |ui| {
                ui.checkbox(&mut self.settings.close_to_tray, "Ao fechar a janela, continuar rodando na bandeja")
                    .on_hover_text("O Elo fica perto do relógio e continua aceitando conexões. Para sair, use \"Sair do Elo\" no menu do ícone.")
                    .on_disabled_hover_text("A bandeja do sistema não está disponível (no GNOME, precisa da extensão AppIndicator).");
            });
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
            ui.label(RichText::new("Atualizações").font(theme::semibold(13.0)).color(p.text));
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Versão instalada: {}", crate::update::CURRENT)).size(12.5).color(p.text2));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!self.updater.busy(), egui::Button::new("Verificar agora")).clicked() {
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
                            ui.label(RichText::new("Verificando…").size(12.0).color(p.text2));
                        });
                    }
                    State::UpToDate => {
                        ui.label(RichText::new("Você está na versão mais recente.").size(12.0).color(p.success));
                    }
                    State::Available { manifest, asset } => {
                        let v = manifest.version.clone();
                        let has = asset.is_some();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Versão {v} disponível.")).size(12.0).color(p.accent_text));
                            if has {
                                if ui.small_button("Baixar e instalar").clicked() {
                                    update_go = true;
                                }
                            } else if ui.small_button("Abrir página de download").clicked() {
                                open_url(crate::update::RELEASES);
                            }
                        });
                    }
                    State::Downloading { version, done, total } => {
                        let f = done.load(std::sync::atomic::Ordering::Relaxed) as f32 / (*total).max(1) as f32;
                        ui.label(RichText::new(format!("Baixando a versão {version}…")).size(12.0).color(p.text2));
                        ui.add(egui::ProgressBar::new(f).desired_height(5.0));
                    }
                    State::Ready { version, .. } => {
                        let v = version.clone();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Versão {v} pronta para instalar.")).size(12.0).color(p.success));
                            if ui.small_button("Reiniciar e atualizar").clicked() {
                                update_restart = true;
                            }
                        });
                    }
                    State::Failed(e) => {
                        ui.add(egui::Label::new(RichText::new(format!("Não foi possível atualizar: {e}")).size(12.0).color(p.danger)).wrap());
                    }
                }
            }
            ui.checkbox(&mut self.settings.auto_update, "Atualizar automaticamente").on_hover_text("Baixa sozinho e instala quando não há sessão em andamento. O Elo reabre em seguida.");
            if let crate::update::Install::Manual(why) = &self.updater.install {
                ui.add(egui::Label::new(RichText::new(format!("Este Elo não se atualiza sozinho: {why}.")).size(11.5).color(p.muted)).wrap());
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("Servidor").font(theme::semibold(13.0)).color(p.text));
            ui.checkbox(&mut self.settings.local_server.enabled, "Rodar o servidor neste computador");
            if self.settings.local_server.enabled {
                let probing = self.probe.is_some();
                let probe_result = self.probe_result.clone();
                let local_running = self.local.server.is_some();
                let ls = &mut self.settings.local_server;
                ui.horizontal(|ui| {
                    theme::field_label(ui, "Porta");
                    ui.add(egui::DragValue::new(&mut ls.port).range(1024..=65535).speed(0.0));
                    ui.add_space(12.0);
                    theme::field_label(ui, "Histórico");
                    ui.add(egui::DragValue::new(&mut ls.retention_days).range(0..=3650).suffix(" dias").speed(1.0)).on_hover_text("Por quantos dias o servidor guarda o histórico de sessões (quem acessou quem, quando e quanto). 0 = para sempre. Esses dados são pessoais (LGPD).");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(local_running, egui::Button::new("Painel do servidor")).on_hover_text("Quem está online, sessões em andamento e histórico").clicked() {
                            open_panel = true;
                        }
                    });
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
fn files_panel(ui: &mut egui::Ui, v: &mut ViewSession, downloads: &std::path::Path) {
    let p = pal(ui);
    let cmd = v.handle.cmd.clone();
    let mut go: Option<String> = None;
    ui.horizontal(|ui| {
        icons::icon(ui, Icon::FolderOpen, 16.0, p.accent);
        let who = if v.target_name.is_empty() { pretty_id(&v.target_id) } else { v.target_name.clone() };
        ui.label(RichText::new(format!("Arquivos de {who}")).font(theme::semibold(14.0)).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(ui, Icon::Close, "Fechar", false).clicked() {
                v.files.open = false;
            }
        });
    });
    ui.add_space(8.0);
    // Barra de caminho.
    let f = &mut v.files;
    let parent = f.listing.as_ref().and_then(|l| l.parent.clone());
    let current = f.listing.as_ref().map(|l| l.path.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        if ui.add_enabled_ui(parent.is_some(), |ui| theme::icon_button(ui, Icon::ArrowUp, "Pasta de cima", true)).inner.clicked() {
            go = parent.clone();
        }
        if theme::icon_button(ui, Icon::Home, "Pasta pessoal", true).clicked() {
            go = Some(String::new());
        }
        if theme::icon_button(ui, Icon::Refresh, "Atualizar", true).clicked() {
            go = Some(current.clone());
        }
        let r = ui.add(egui::TextEdit::singleline(&mut f.path).font(theme::mono(12.0)).hint_text("caminho no outro computador").desired_width(ui.available_width()));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            go = Some(f.path.clone());
        }
    });
    ui.add_space(6.0);
    if let Some(e) = f.listing.as_ref().and_then(|l| l.error.clone()) {
        ui.add(egui::Label::new(RichText::new(e).size(12.0).color(p.danger)).wrap());
    }
    // Lista da pasta.
    let footer = 230.0;
    let list_h = (ui.available_height() - footer).max(120.0);
    egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
            if f.loading && f.listing.is_none() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.spinner());
                return;
            }
            let Some(l) = &f.listing else { return };
            let mut shown = 0;
            for e in l.entries.iter().filter(|e| f.hidden || !e.name.starts_with('.')) {
                shown += 1;
                let sel = f.selected.contains(&e.name);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
                let fill = if sel { p.selected } else if resp.hovered() { p.hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                let ic = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(15.0));
                icons::paint(ui.painter(), ic, if e.dir { Icon::Folder } else { Icon::File }, if e.dir { p.accent } else { p.text2 });
                let size = if e.dir { String::new() } else { human_bytes(e.size) };
                let sw = ui.painter().layout_no_wrap(size.clone(), theme::mono(11.0), p.muted).size().x;
                let mut job = egui::text::LayoutJob::simple_singleline(e.name.clone(), egui::FontId::proportional(13.0), p.text);
                job.wrap = egui::text::TextWrapping { max_width: (rect.width() - 52.0 - sw).max(40.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                let g = ui.fonts_mut(|fo| fo.layout_job(job));
                ui.painter().galley(egui::pos2(rect.left() + 32.0, rect.center().y - g.size().y / 2.0), g, p.text);
                ui.painter().text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, size, theme::mono(11.0), p.muted);
                if resp.double_clicked() {
                    if e.dir {
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
                ui.add_space(16.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Pasta vazia").size(12.5).color(p.muted)));
            }
        });
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let n = f.selected.len();
        let label = if n > 1 { format!("Baixar {n} arquivos") } else { "Baixar".to_string() };
        if ui.add_enabled_ui(n > 0, |ui| theme::primary(ui, Some(Icon::Download), &label, None)).inner.on_disabled_hover_text("Selecione arquivos (clique). Clique duplo baixa direto.").clicked() {
            for name in f.selected.drain(..) {
                let _ = cmd.send(Cmd::Download(remote_join(&current, &name)));
            }
        }
        if ui.add_enabled_ui(!current.is_empty(), |ui| theme::secondary(ui, Some(Icon::Upload), "Enviar para cá…")).inner.on_hover_text("Envia arquivos deste computador para a pasta aberta. Também dá para arrastar para a janela.").clicked()
            && let Some(list) = rfd::FileDialog::new().pick_files()
        {
            for path in list {
                let _ = cmd.send(Cmd::Upload(path, current.clone()));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.checkbox(&mut f.hidden, "Ocultos");
        });
    });
    if let Some(path) = go {
        f.loading = true;
        let _ = cmd.send(Cmd::ListDir(path));
    }
    // Transferências.
    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Transferências").font(theme::semibold(12.5)).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("Abrir recebidos").on_hover_text(downloads.display().to_string()).clicked() {
                let _ = std::fs::create_dir_all(downloads);
                open_path(downloads);
            }
        });
    });
    egui::ScrollArea::vertical().id_salt("transferencias").auto_shrink([false, false]).show(ui, |ui| {
        if v.transfers.is_empty() {
            ui.label(RichText::new("Nada ainda. Arquivos baixados vão para a pasta de recebidos.").size(11.5).color(p.muted));
        }
        for t in v.transfers.iter().rev().take(20) {
            let pct = if t.total > 0 { t.done as f32 / t.total as f32 } else { 1.0 };
            ui.horizontal(|ui| {
                icons::icon(ui, if t.incoming { Icon::Download } else { Icon::Upload }, 13.0, if t.finished { p.success } else { p.text2 });
                let status = if t.finished { "concluído".to_string() } else { format!("{:.0}% de {}", pct * 100.0, human_bytes(t.total)) };
                ui.add(egui::Label::new(RichText::new(&t.name).size(12.0).color(p.text)).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if t.finished && t.incoming
                        && let Some(path) = &t.path
                        && ui.small_button("Abrir").clicked()
                    {
                        open_path(path);
                    }
                    ui.label(RichText::new(status).size(11.0).color(p.muted));
                });
            });
            if !t.finished {
                ui.add(egui::ProgressBar::new(pct).desired_height(4.0));
            }
        }
    });
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

/// Botão principal largo (toda a largura), com o texto centralizado.
fn big_button(ui: &mut egui::Ui, p: &Palette, text: &str, enabled: bool) -> egui::Response {
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
    star: Option<String>,
    start_rename: Option<String>,
    renamed: Option<(String, String)>,
}

/// Cor do avatar por sistema.
fn os_color(os: &str) -> Color32 {
    match os {
        "macOS" => Color32::from_rgb(110, 110, 120),
        "Windows" => Color32::from_rgb(14, 140, 233),
        "Linux" => Color32::from_rgb(226, 150, 30),
        _ => Color32::from_rgb(120, 130, 150),
    }
}

/// Cartão de um computador: avatar, nome, ID e ações (estrela, renomear).
fn tile(ui: &mut egui::Ui, p: &Palette, id: &str, name: &str, sub_name: &str, os: &str, fav: bool, renaming: Option<&mut String>, out: &mut TileOut) {
    // O fundo clicável vem primeiro; os botões por cima recebem os próprios cliques.
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(12), if hovered { p.hover } else { p.surface }, Stroke::new(1.0, if hovered { theme::alpha(p.accent, 0.55) } else { p.border }), egui::StrokeKind::Inside);
    let av = egui::pos2(rect.left() + 32.0, rect.center().y);
    painter.circle_filled(av, 19.0, os_color(os));
    icons::paint(painter, egui::Rect::from_center_size(av, Vec2::splat(20.0)), Icon::Monitor, Color32::WHITE);
    let text_left = rect.left() + 62.0;
    let text_w = (rect.right() - 74.0 - text_left).max(40.0);
    let sub = if sub_name.is_empty() { format!("{} · {os}", pretty_id(id)) } else { format!("{sub_name} · {}", pretty_id(id)) };
    match renaming {
        Some(text) => {
            let r = ui.put(egui::Rect::from_min_size(egui::pos2(text_left, rect.center().y - 14.0), Vec2::new(text_w, 28.0)), egui::TextEdit::singleline(text).hint_text("Nome para este computador"));
            r.request_focus();
            if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out.renamed = Some((id.to_string(), text.trim().to_string()));
            }
        }
        None => {
            let one_line = |t: &str, font: egui::FontId, color: Color32| {
                let mut job = egui::text::LayoutJob::simple_singleline(t.to_string(), font, color);
                job.wrap = egui::text::TextWrapping { max_width: text_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let g1 = one_line(name, theme::semibold(13.5), p.text);
            let g2 = one_line(&sub, theme::mono(11.0), p.muted);
            painter.galley(egui::pos2(text_left, rect.center().y - 18.0), g1, p.text);
            painter.galley(egui::pos2(text_left, rect.center().y + 3.0), g2, p.muted);
        }
    }
    // Ações à direita.
    let star = egui::Rect::from_center_size(egui::pos2(rect.right() - 24.0, rect.center().y), Vec2::splat(28.0));
    let sr = ui.put(star, egui::Button::new("").frame(false)).on_hover_text(if fav { "Tirar dos favoritos" } else { "Salvar nos favoritos" });
    icons::paint(ui.painter(), star.shrink(6.0), Icon::Star, if fav { p.warning } else if sr.hovered() { p.text } else { p.muted });
    if sr.clicked() {
        out.star = Some(id.to_string());
    }
    if fav && (hovered || sr.hovered()) {
        let edit = egui::Rect::from_center_size(egui::pos2(rect.right() - 52.0, rect.center().y), Vec2::splat(28.0));
        let er = ui.put(edit, egui::Button::new("").frame(false)).on_hover_text("Renomear");
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
        v.files.open = false;
    }
    let files = ui.add_enabled_ui(v.permissions.files, |ui| ui.selectable_label(v.files.open, "Arquivos")).inner;
    if files.on_hover_text("Ver as pastas do outro computador, baixar e enviar arquivos (dá para arrastar arquivos para a janela)").on_disabled_hover_text("O outro lado não liberou a transferência de arquivos").clicked() {
        v.files.open = !v.files.open;
        v.chat_open = false;
        if v.files.open && v.files.listing.is_none() {
            v.files.loading = true;
            let _ = v.handle.cmd.send(Cmd::ListDir(String::new()));
        }
    }
    if ui.add_enabled(v.permissions.control, egui::Button::new("Ctrl+Alt+Del")).clicked() {
        let _ = v.handle.cmd.send(Cmd::SecureAttention);
    }
    if ui.add_enabled(v.permissions.control, egui::Button::new("Reiniciar")).on_hover_text("Reinicia o outro computador e reconecta quando ele voltar").clicked() {
        v.confirm_restart = true;
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
