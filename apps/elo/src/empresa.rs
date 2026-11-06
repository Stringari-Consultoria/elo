//! Empresas: a aba "Clientes" do técnico (pedidos de ajuda, empresas,
//! computadores, convites e técnicos), a conta de técnico nas configurações e
//! a tela do Elo Cliente (só recebe conexões e pede ajuda).

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Vec2};
use elo_core::client::Command;
use elo_core::net::ServerCfg;
use elo_proto::ctrl::{DeviceEntry, Invite, Manage, pretty_id};
use stringari_ui::icons::{self, Icon};
use stringari_ui::theme::{self, Palette, pal};

use crate::app::{EloApp, Status};
use crate::ui::{ago, big_button, card, copy, os_color};

/// Onde baixar o Elo Cliente (vai na mensagem do convite).
const DOWNLOAD: &str = "https://github.com/Stringari-Consultoria/elo/releases/latest";

/// Estado da aba Clientes.
#[derive(Default)]
pub struct ClientsUi {
    /// Empresa escolhida: `None` = todas; `Some("")` = sem empresa.
    pub company: Option<String>,
    pub search: String,
    pub edit: Option<DeviceEdit>,
    /// Nome da empresa nova sendo digitado.
    pub new_company: Option<String>,
    /// Empresa sendo renomeada: (id, nome).
    pub rename: Option<(String, String)>,
    /// Convite aberto: empresa e endereço escolhido.
    pub invite: Option<(String, usize)>,
    pub techs_open: bool,
    pub new_tech: NewTech,
    /// Confirmação de algo destrutivo: (texto, ação).
    pub confirm: Option<(String, Manage)>,
}

pub struct DeviceEdit {
    pub id: String,
    pub alias: String,
    pub notes: String,
    pub company: Option<String>,
}

#[derive(Default)]
pub struct NewTech {
    pub user: String,
    pub name: String,
    pub password: String,
    pub admin: bool,
}

impl EloApp {
    fn manage(&self, m: Manage) {
        let _ = self.control.tx.send(Command::Manage(m));
    }

    /// Endereços pelos quais os clientes chegam a este servidor (para o convite).
    pub fn invite_targets(&self) -> Vec<(String, ServerCfg)> {
        let mut v = Vec::new();
        let ls = &self.settings.local_server;
        if ls.enabled {
            if let Some(w) = ls.public_ws() {
                v.push((t!("pela internet ({})", w), ServerCfg { address: w.to_string(), tls: true, pin: String::new() }));
            }
            if ls.lan {
                let a = self.local.lan_address();
                v.push((t!("só na rede local ({})", a), ServerCfg { address: a, tls: true, pin: self.local.fingerprint.clone() }));
            }
        } else if !self.settings.server.address.trim().is_empty() {
            v.push((t!("servidor configurado ({})", self.settings.server.address.trim()), self.settings.server.clone()));
        }
        v
    }

    // ── Conta de técnico (nas configurações) ──────────────────────────

    pub fn tech_section(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(RichText::new(t!("Conta de técnico")).font(theme::semibold(13.0)).color(p.text));
        if let Some(t) = self.settings.tech.clone() {
            ui.horizontal(|ui| {
                theme::dot(ui, if self.overview.is_some() { p.success } else { p.warning }, 8.0);
                ui.label(RichText::new(format!("{} ({}){}", t.name, t.user, if t.admin { t!(" · administrador") } else { "" })).size(12.5).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(t!("Sair")).clicked() {
                        self.tech_logout();
                    }
                });
            });
            ui.label(RichText::new(t!("A aba Clientes mostra as empresas atendidas, os computadores delas e os pedidos de ajuda.")).size(11.5).color(p.muted));
            return;
        }
        if !self.features.iter().any(|f| f == "empresas") {
            ui.label(RichText::new(t!("O servidor em uso não tem contas de técnico (é de uma versão antiga ou está offline).")).size(11.5).color(p.muted));
            return;
        }
        ui.label(RichText::new(t!("Para atender empresas: ver os computadores delas, os pedidos de ajuda e mandar convites do Elo Cliente.")).size(11.5).color(p.muted));
        let f = &mut self.tech_form;
        if f.needs_setup {
            ui.label(RichText::new(t!("Este servidor ainda não tem técnicos. Crie a conta de administrador com o código de primeiro acesso (aparece nos registros do servidor e no arquivo primeiro-acesso.txt da pasta de dados).")).size(11.5).color(p.warning));
            theme::field_label(ui, t!("Código de primeiro acesso"));
            ui.add(egui::TextEdit::singleline(&mut f.setup).hint_text("abcd-efgh").font(theme::mono(12.5)).desired_width(f32::INFINITY));
            theme::field_label(ui, t!("Seu nome"));
            ui.add(egui::TextEdit::singleline(&mut f.name).hint_text(t!("como aparece para os clientes")).desired_width(f32::INFINITY));
        }
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 8.0) / 2.0;
            ui.vertical(|ui| {
                ui.set_width(w);
                theme::field_label(ui, t!("Usuário"));
                ui.add(egui::TextEdit::singleline(&mut f.user).desired_width(f32::INFINITY));
            });
            ui.vertical(|ui| {
                ui.set_width(w);
                theme::field_label(ui, t!("Senha"));
                ui.add(egui::TextEdit::singleline(&mut f.password).password(true).hint_text(t!("8 caracteres ou mais")).desired_width(f32::INFINITY));
            });
        });
        let ready = !f.user.trim().is_empty() && f.password.chars().count() >= 8 && (!f.needs_setup || !f.setup.trim().is_empty());
        let label = if f.busy {
            t!("Entrando…"
        )
        } else if f.needs_setup {
            t!("Criar administrador e entrar"
        )
        } else {
            t!("Entrar"
        )
        };
        let busy = f.busy;
        if ui.add_enabled(ready && !busy, egui::Button::new(label)).clicked() {
            self.tech_login();
        }
    }

    // ── Aba Clientes ──────────────────────────────────────────────────

    pub fn clients_page(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let Some(o) = self.overview.clone() else { return ; };
        let admin = o.me.admin;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(1040.0);
            // Cabeçalho.
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(t!("Clientes")).font(theme::semibold(22.0)).color(p.text));
                    let plural = |n: usize, um: &str, varios: &str| if n == 1 { t!("1 {}", um) } else { t!("{} {}", n, varios) };
                    let n_dev = o.devices.iter().filter(|d| d.company.is_some()).count();
                    ui.label(RichText::new(t!("{} · {} de clientes · você entrou como {}", plural(o.companies.len(), t!("empresa"), t!("empresas")), plural(n_dev, t!("computador"), t!("computadores")), o.me.name)).size(12.5).color(p.text2));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if admin && theme::secondary(ui, Some(Icon::Shield), t!("Técnicos")).clicked() {
                        self.clients.techs_open = true;
                    }
                    if theme::primary(ui, Some(Icon::Plus), t!("Nova empresa"), None).clicked() {
                        self.clients.new_company = Some(String::new());
                    }
                    ui.add(egui::TextEdit::singleline(&mut self.clients.search).hint_text(t!("Buscar nome, ID ou observação")).desired_width(220.0));
                });
            });
            ui.add_space(16.0);

            // Pedidos de ajuda.
            if !o.requests.is_empty() {
                ui.label(RichText::new(t!("Pedidos de ajuda ({})", o.requests.len())).font(theme::semibold(14.0)).color(p.text));
                ui.add_space(6.0);
                let mut take = None;
                for r in &o.requests {
                    let company = r.company.as_ref().and_then(|c| o.companies.iter().find(|x| x.id == *c)).map(|c| c.name.clone()).unwrap_or_default();
                    egui::Frame::new().fill(theme::alpha(p.warning, 0.10)).stroke(Stroke::new(1.0, theme::alpha(p.warning, 0.45))).corner_radius(CornerRadius::same(12)).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            icons::icon(ui, Icon::Alert, 16.0, p.warning);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("{}{}", r.name, if company.is_empty() { String::new() } else { t!(" · {}", company) })).font(theme::semibold(13.0)).color(p.text));
                                let msg = if r.message.is_empty() { t!("(sem mensagem)").to_string() } else { format!("“{}”", r.message) };
                                ui.add(egui::Label::new(RichText::new(t!("{} · {} · ID {}", msg, ago(r.since), pretty_id(&r.device))).size(12.0).color(p.text2)).wrap());
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                match &r.taken_by {
                                    Some(n) if *n != o.me.name => {
                                        ui.label(RichText::new(t!("{} está atendendo", n)).size(12.0).color(p.muted));
                                    }
                                    _ => {
                                        if theme::primary(ui, None, if r.taken_by.is_some() { t!("Conectar") } else { t!("Atender") }, None).on_hover_text(t!("Marca o pedido como seu e pede acesso ao computador (o cliente aceita na tela)")).clicked() {
                                            take = Some(r.device.clone());
                                        }
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
                if let Some(d) = take {
                    self.manage(Manage::TakeRequest { device: d.clone() });
                    self.connect(d, String::new());
                }
                ui.add_space(12.0);
            }

            // Empresas (esquerda) e computadores (direita).
            let full = ui.available_width();
            let left_w = 230.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    let count = |c: Option<&str>| {
                        let list: Vec<&DeviceEntry> = o.devices.iter().filter(|d| c.is_none_or(|c| d.company.as_deref().unwrap_or("") == c)).collect();
                        (list.iter().filter(|d| d.online).count(), list.len())
                    };
                    let mut item = |ui: &mut egui::Ui, key: Option<String>, name: &str| {
                        let (on, all) = count(key.as_deref());
                        let sel = self.clients.company == key;
                        let r = egui::Frame::new()
                            .fill(if sel { p.selected } else { Color32::TRANSPARENT })
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.add(egui::Label::new(RichText::new(name).size(13.0).color(p.text)).truncate());
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{on}/{all}")).font(theme::mono(11.0)).color(p.muted)).on_hover_text(t!("online / total"));
                                    });
                                });
                            })
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            self.clients.company = key;
                        }
                    };
                    item(ui, None, t!("Todos os computadores"));
                    for c in &o.companies {
                        item(ui, Some(c.id.clone()), &c.name);
                    }
                    item(ui, Some(String::new()), t!("Sem empresa"));
                });
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.set_width(full - left_w - 14.0);
                    self.devices_pane(ui, &p, &o);
                });
            });
        });
    }

    fn devices_pane(&mut self, ui: &mut egui::Ui, p: &Palette, o: &elo_proto::ctrl::Overview) {
        let sel = self.clients.company.clone();
        // Cabeçalho da empresa escolhida.
        if let Some(cid) = sel.as_deref().filter(|c| !c.is_empty())
            && let Some(c) = o.companies.iter().find(|c| c.id == cid)
        {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&c.name).font(theme::semibold(16.0)).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(ui, Icon::Trash, t!("Apagar a empresa (os computadores ficam sem empresa)"), true).clicked() {
                        self.clients.confirm = Some((t!("Apagar a empresa {}? Os computadores dela ficam sem empresa e passam a aceitar conexões de qualquer um que tenha a senha deles.", c.name), Manage::DeleteCompany { id: c.id.clone() }));
                    }
                    if theme::icon_button(ui, Icon::Refresh, t!("Trocar o código de convite (o antigo deixa de valer para computadores novos)"), true).clicked() {
                        self.clients.confirm = Some((t!("Trocar o código de convite de {}? O código antigo deixa de valer para instalações novas; os computadores já ligados continuam.", c.name), Manage::NewKey { id: c.id.clone() }));
                    }
                    if theme::icon_button(ui, Icon::Edit, t!("Renomear"), true).clicked() {
                        self.clients.rename = Some((c.id.clone(), c.name.clone()));
                    }
                    if theme::primary(ui, Some(Icon::Link), t!("Convidar computador"), None).on_hover_text(t!("Código para instalar o Elo Cliente num computador desta empresa")).clicked() {
                        self.clients.invite = Some((c.id.clone(), 0));
                    }
                });
            });
            ui.add_space(8.0);
        }
        let q = self.clients.search.trim().to_lowercase();
        let list: Vec<&DeviceEntry> = o
            .devices
            .iter()
            .filter(|d| match sel.as_deref() {
                None => true,
                Some("") => d.company.is_none(),
                Some(c) => d.company.as_deref() == Some(c),
            })
            .filter(|d| { q.is_empty() || [&d.name, &d.alias, &d.notes, &d.id].iter().any(|t| t.to_lowercase().contains(&q)) || pretty_id(&d.id).contains(&q)})
            .collect();
        if o.companies.is_empty() {
            card(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(t!("Nenhuma empresa ainda")).font(theme::semibold(14.0)).color(p.text));
                ui.add(egui::Label::new(RichText::new(t!("Crie uma empresa (\"Nova empresa\"), clique em \"Convidar computador\" e mande o código para o cliente. Ele instala o Elo Cliente, cola o código e o computador aparece aqui. O Elo Cliente só recebe conexões, sempre com aceite na tela, e tem o botão \"Pedir ajuda\".")).size(12.5).color(p.text2)).wrap());
            });
            ui.add_space(10.0);
        }
        if list.is_empty() {
            ui.label(RichText::new(if q.is_empty() { t!("Nenhum computador aqui ainda.") } else { t!("Nada encontrado.") }).size(12.5).color(p.muted));
            return;
        }
        let mut connect = None;
        let mut edit = None;
        for d in list {
            let company = d.company.as_ref().and_then(|c| o.companies.iter().find(|x| x.id == *c)).map(|c| c.name.clone());
            card(p).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 16.0, os_color(&d.os));
                    icons::paint(ui.painter(), r.shrink(8.0), Icon::Monitor, Color32::WHITE);
                    let dot = r.right_bottom() - Vec2::splat(3.0);
                    ui.painter().circle_filled(dot, 5.5, p.surface);
                    ui.painter().circle_filled(dot, 4.0, if d.online { p.success } else { p.muted });
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        let title = if d.alias.is_empty() { d.name.clone() } else { d.alias.clone() };
                        ui.label(RichText::new(title).font(theme::semibold(13.0)).color(p.text));
                        let mut sub = vec![pretty_id(&d.id)];
                        if !d.alias.is_empty() && !d.name.is_empty() {
                            sub.push(d.name.clone());
                        }
                        if !d.os.is_empty() {
                            sub.push(d.os.clone());
                        }
                        if sel.is_none()
                            && let Some(c) = &company
                        {
                            sub.push(c.clone());
                        }
                        sub.push(if d.online { t!("online").into() } else if d.last_seen > 0 { t!("visto {}", ago(d.last_seen)) } else { t!("offline").into() });
                        ui.label(RichText::new(sub.join(" · ")).size(11.5).color(p.text2));
                        if !d.notes.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&d.notes).size(11.5).color(p.muted)).truncate());
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::icon_button(ui, Icon::Edit, t!("Nome, empresa e observações"), true).clicked() {
                            edit = Some(d.clone());
                        }
                        if ui.add_enabled(d.online, egui::Button::new(t!("Conectar"))).on_hover_text(t!("Pede acesso (o cliente aceita na tela)")).on_disabled_hover_text(t!("Offline")).clicked() {
                            connect = Some(d.id.clone());
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
        if let Some(id) = connect {
            self.connect(id, String::new());
        }
        if let Some(d) = edit {
            self.clients.edit = Some(DeviceEdit { id: d.id, alias: d.alias, notes: d.notes, company: d.company });
        }
    }

    /// Janelas da aba Clientes.
    pub fn clients_dialogs(&mut self, ctx: &egui::Context) {
        let Some(o) = self.overview.clone() else { return ; };
        let p = if self.settings.dark { theme::DARK } else { theme::LIGHT };
        let frame = egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(22));
        // Empresa nova.
        if let Some(name) = self.clients.new_company.as_mut() {
            let (mut ok, mut close) = (false, false);
            egui::Modal::new(egui::Id::unique("nova-empresa")).frame(frame).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(t!("Nova empresa")).font(theme::semibold(16.0)).color(p.text));
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(name).hint_text(t!("Nome da empresa cliente")).desired_width(f32::INFINITY));
                r.request_focus();
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = theme::primary(ui, None, t!("Criar"), None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    close = theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
            if ok && !name.trim().is_empty() {
                let n = name.trim().to_string();
                self.manage(Manage::CreateCompany { name: n });
                self.clients.new_company = None;
            } else if close {
                self.clients.new_company = None;
            }
        }
        // Renomear empresa.
        if let Some((id, name)) = self.clients.rename.as_mut() {
            let (mut ok, mut close) = (false, false);
            egui::Modal::new(egui::Id::unique("renomear-empresa")).frame(frame).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(t!("Renomear empresa")).font(theme::semibold(16.0)).color(p.text));
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = theme::primary(ui, None, t!("Salvar"), None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    close = theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
            if ok {
                let m = Manage::RenameCompany { id: id.clone(), name: name.trim().to_string() };
                self.manage(m);
                self.clients.rename = None;
            } else if close {
                self.clients.rename = None;
            }
        }
        // Convite.
        if let Some((cid, choice)) = self.clients.invite.clone() {
            let targets = self.invite_targets();
            let company = o.companies.iter().find(|c| c.id == cid).cloned();
            let mut close = company.is_none();
            let mut pick = choice;
            egui::Modal::new(egui::Id::unique("convite")).frame(frame).show(ctx, |ui| {
                ui.set_width(520.0);
                let Some(c) = &company else { return };
                ui.label(RichText::new(t!("Convidar um computador de {}", c.name)).font(theme::semibold(16.0)).color(p.text));
                ui.add_space(6.0);
                if targets.is_empty() {
                    ui.add(egui::Label::new(RichText::new(t!("Configure como os clientes chegam a este servidor: em Configurações → Servidor, preencha o endereço público (wss://…) ou ligue as conexões da rede.")).size(12.5).color(p.warning)).wrap());
                } else {
                    if targets.len() > 1 {
                        theme::field_label(ui, t!("Os clientes chegam ao servidor"));
                        for (i, (label, _)) in targets.iter().enumerate() {
                            ui.radio_value(&mut pick, i, label.as_str());
                        }
                    } else {
                        ui.label(RichText::new(t!("Servidor: {}", targets[0].0)).size(12.0).color(p.text2));
                    }
                    let t = &targets[pick.min(targets.len() - 1)].1;
                    let code = Invite { address: t.address.clone(), tls: t.tls, pin: t.pin.clone(), company: c.id.clone(), key: c.key.clone(), name: c.name.clone() }.encode();
                    ui.add_space(8.0);
                    theme::field_label(ui, t!("Código de convite"));
                    egui::Frame::new().fill(p.sunken).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(&code).font(theme::mono(11.5)).color(p.text)).wrap());
                    });
                    ui.add_space(8.0);
                    let message = format!(
                        "Para receber suporte neste computador:\n1. Baixe e instale o Elo Cliente: {DOWNLOAD}\n2. Abra e cole este código de convite:\n{code}\n\nNinguém acessa o computador sem você aceitar na tela. Quando precisar, use o botão \"Pedir ajuda\"."
                    );
                    ui.horizontal(|ui| {
                        if theme::primary(ui, Some(Icon::Copy), t!("Copiar mensagem para o cliente"), None).clicked() {
                            copy(ui, &message);
                            self.toast(t!("Mensagem com o link e o código copiada."));
                        }
                        if theme::secondary(ui, Some(Icon::Copy), t!("Copiar só o código")).clicked() {
                            copy(ui, &code);
                            self.toast(t!("Código de convite copiado."));
                        }
                    });
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(t!("Para instalar em muitas máquinas, a TI pode salvar o código num arquivo convite.txt: em C:\\ProgramData\\Elo\\ (Windows), /etc/elo/ (Linux) ou /Library/Application Support/Elo/ (Mac). O Elo Cliente lê sozinho na primeira abertura.")).size(11.5).color(p.muted)).wrap());
                }
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = theme::secondary(ui, None, t!("Fechar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
            self.clients.invite = if close { None } else { Some((cid, pick)) };
        }
        // Editar computador.
        if let Some(e) = self.clients.edit.as_mut() {
            let (mut ok, mut close, mut forget) = (false, false, false);
            egui::Modal::new(egui::Id::unique("editar-computador")).frame(frame).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(RichText::new(t!("Computador {}", pretty_id(&e.id))).font(theme::semibold(16.0)).color(p.text));
                ui.add_space(8.0);
                theme::field_label(ui, t!("Nome (como aparece para os técnicos)"));
                ui.add(egui::TextEdit::singleline(&mut e.alias).hint_text(t!("vazio = o nome que o computador informa")).desired_width(f32::INFINITY));
                theme::field_label(ui, t!("Empresa"));
                let current = e.company.as_ref().and_then(|c| o.companies.iter().find(|x| x.id == *c)).map(|c| c.name.clone()).unwrap_or_else(|| t!("Sem empresa").into());
                egui::ComboBox::from_id_salt("empresa-computador").selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut e.company, None, t!("Sem empresa"));
                    for c in &o.companies {
                        ui.selectable_value(&mut e.company, Some(c.id.clone()), &c.name);
                    }
                });
                theme::field_label(ui, t!("Observações"));
                ui.add(egui::TextEdit::multiline(&mut e.notes).desired_rows(4).hint_text(t!("setor, usuário, impressora, senha do Wi-Fi não!…")).desired_width(f32::INFINITY));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = theme::primary(ui, None, t!("Salvar"), None).clicked();
                    close = theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        forget = theme::danger_subtle(ui, None, t!("Tirar da lista")).on_hover_text(t!("Some do painel; se o Elo dele abrir de novo, volta sem empresa")).clicked();
                    });
                });
            });
            if ok {
                let m = Manage::SetDevice { id: e.id.clone(), company: e.company.clone(), alias: e.alias.clone(), notes: e.notes.clone() };
                self.manage(m);
                self.clients.edit = None;
            } else if forget {
                let id = e.id.clone();
                self.clients.edit = None;
                self.clients.confirm = Some((t!("Tirar o computador {} da lista?", pretty_id(&id)), Manage::ForgetDevice { id }));
            } else if close {
                self.clients.edit = None;
            }
        }
        // Técnicos (administradores).
        if self.clients.techs_open {
            let mut close = false;
            let mut create = false;
            let mut remove = None;
            egui::Modal::new(egui::Id::unique("tecnicos")).frame(frame).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new(t!("Técnicos")).font(theme::semibold(16.0)).color(p.text));
                ui.label(RichText::new(t!("Cada técnico entra no Elo com usuário e senha (Configurações → Conta de técnico) e vê todas as empresas.")).size(12.0).color(p.text2));
                ui.add_space(8.0);
                for t in &o.techs {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} ({}){}", t.name, t.user, if t.admin { t!(" · administrador") } else { "" })).size(12.5).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if t.user != o.me.user && ui.small_button(t!("Remover")).clicked() {
                                remove = Some(t.user.clone());
                            }
                        });
                    });
                }
                ui.separator();
                ui.label(RichText::new(t!("Novo técnico")).font(theme::semibold(13.0)).color(p.text));
                let n = &mut self.clients.new_tech;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut n.user).hint_text(t!("usuário")).desired_width(130.0));
                    ui.add(egui::TextEdit::singleline(&mut n.name).hint_text("nome").desired_width(150.0));
                    ui.add(egui::TextEdit::singleline(&mut n.password).password(true).hint_text(t!("senha (8+)")).desired_width(ui.available_width()));
                });
                ui.checkbox(&mut n.admin, t!("Administrador (cria e remove técnicos)"));
                let ready = !n.user.trim().is_empty() && n.password.chars().count() >= 8;
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    create = ui.add_enabled(ready, egui::Button::new(t!("Criar técnico"))).clicked();
                    close = theme::secondary(ui, None, t!("Fechar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
            if create {
                let n = std::mem::take(&mut self.clients.new_tech);
                self.manage(Manage::CreateTech { user: n.user, name: n.name, password: n.password, admin: n.admin });
            }
            if let Some(user) = remove {
                self.clients.confirm = Some((t!("Remover o técnico {}? Ele sai na hora de todos os apps.", user), Manage::DeleteTech { user }));
            }
            if close {
                self.clients.techs_open = false;
            }
        }
        // Confirmação.
        if let Some((text, action)) = self.clients.confirm.clone() {
            let (mut ok, mut close) = (false, false);
            egui::Modal::new(egui::Id::unique("confirmar-empresa")).frame(frame).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.add(egui::Label::new(RichText::new(text).size(13.5).color(p.text)).wrap());
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = theme::danger_button(ui, t!("Confirmar")).clicked();
                    close = theme::secondary(ui, None, t!("Cancelar")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
            if ok {
                if matches!(action, Manage::DeleteCompany { .. }) {
                    self.clients.company = None;
                }
                self.manage(action);
                self.clients.confirm = None;
            } else if close {
                self.clients.confirm = None;
            }
        }
    }

    // ── Elo Cliente ───────────────────────────────────────────────────

    pub fn client_home(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width().min(560.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(w);
                ui.add_space(12.0);
                let Some(company) = self.settings.company.clone() else {
                    self.client_setup(ui, &p);
                    return;
                };
                ui.label(RichText::new(crate::variant::app_name()).font(theme::semibold(22.0)).color(p.text));
                ui.label(RichText::new(t!("Suporte para {}", company.name)).size(13.0).color(p.text2));
                ui.add_space(18.0);
                // ID.
                card(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(t!("SEU ID")).font(theme::semibold(11.0)).extra_letter_spacing(0.8).color(p.muted));
                        match self.status.clone() {
                            Status::Online(id) => {
                                ui.horizontal(|ui| {
                                    let text = pretty_id(&id);
                                    let tw = ui.painter().layout_no_wrap(text.clone(), theme::mono(34.0), p.text).size().x + 36.0;
                                    ui.add_space(((ui.available_width() - tw) / 2.0).max(0.0));
                                    ui.label(RichText::new(&text).font(theme::mono(34.0)).color(p.text));
                                    if theme::icon_button(ui, Icon::Copy, t!("Copiar ID"), false).clicked() {
                                        copy(ui, &text);
                                        self.toast(t!("ID copiado."));
                                    }
                                });
                            }
                            Status::Connecting => {
                                ui.spinner();
                            }
                            Status::Offline(r) => {
                                ui.add(egui::Label::new(RichText::new(t!("Sem conexão com o servidor da empresa: {}", r)).size(12.0).color(p.danger)).wrap());
                            }
                        }
                    });
                });
                ui.add_space(14.0);
                // Pedir ajuda.
                card(&p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match self.help.clone() {
                        None => {
                            ui.label(RichText::new(t!("Precisa de ajuda?")).font(theme::semibold(16.0)).color(p.text));
                            ui.label(RichText::new(t!("Conte o que está acontecendo (opcional) e chame um técnico.")).size(12.5).color(p.text2));
                            ui.add_space(10.0);
                            ui.add(egui::TextEdit::multiline(&mut self.help_text).desired_rows(3).hint_text(t!("Ex.: a impressora não imprime")).desired_width(f32::INFINITY));
                            ui.add_space(12.0);
                            let online = matches!(self.status, Status::Online(_));
                            if big_button(ui, &p, t!("Pedir ajuda"), online).clicked() {
                                let t = std::mem::take(&mut self.help_text);
                                let _ = self.control.tx.send(Command::RequestHelp(t));
                            }
                        }
                        Some(None) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new(t!("Pedido enviado. Os técnicos já foram avisados.")).font(theme::semibold(14.0)).color(p.text));
                            });
                            ui.label(RichText::new(t!("Deixe o Elo aberto. Quando aparecer o pedido de acesso, confira o nome do técnico e aceite.")).size(12.5).color(p.text2));
                            ui.add_space(8.0);
                            if theme::secondary(ui, None, t!("Cancelar pedido")).clicked() {
                                let _ = self.control.tx.send(Command::CancelHelp);
                                self.help = None;
                            }
                        }
                        Some(Some(name)) => {
                            ui.horizontal(|ui| {
                                icons::icon(ui, Icon::Check, 16.0, p.success);
                                ui.label(RichText::new(t!("{} vai atender você.", name)).font(theme::semibold(14.0)).color(p.text));
                            });
                            ui.label(RichText::new(t!("Quando aparecer o pedido de acesso, confira se o nome é esse e aceite.")).size(12.5).color(p.text2));
                        }
                    }
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let text = t!("Ninguém acessa este computador sem você aceitar na tela, e você pode encerrar a qualquer momento.");
                    let tw = ui.painter().layout_no_wrap(text.into(), egui::FontId::proportional(11.5), p.muted).size().x + 18.0;
                    ui.add_space(((ui.available_width() - tw) / 2.0).max(0.0));
                    icons::icon(ui, Icon::Lock, 12.0, p.muted);
                    ui.label(RichText::new(text).size(11.5).color(p.muted));
                });
            });
        });
    }

    /// Primeira abertura do Elo Cliente: colar o convite.
    fn client_setup(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(30.0);
        theme::logo(ui, 44.0, Icon::Remote);
        ui.add_space(10.0);
        ui.label(RichText::new(crate::variant::app_name()).font(theme::semibold(22.0)).color(p.text));
        ui.label(RichText::new(t!("Cole o código de convite enviado pela empresa que dá suporte a este computador.")).size(13.0).color(p.text2));
        ui.add_space(16.0);
        card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::TextEdit::multiline(&mut self.invite_input).desired_rows(3).font(theme::mono(12.0)).hint_text("ELO1-…").desired_width(f32::INFINITY));
            ui.add_space(12.0);
            let valid = Invite::decode(&self.invite_input);
            if let Some(i) = &valid {
                ui.label(RichText::new(t!("Empresa: {}", i.name)).size(12.5).color(p.success));
                ui.add_space(6.0);
            } else if !self.invite_input.trim().is_empty() {
                ui.label(RichText::new(t!("Esse código não parece completo: confira se copiou tudo (começa com ELO1-).")).size(12.0).color(p.danger));
                ui.add_space(6.0);
            }
            if big_button(ui, p, t!("Conectar à empresa"), valid.is_some()).clicked() {
                let text = std::mem::take(&mut self.invite_input);
                if let Err(e) = self.join_invite(&text) {
                    self.error(e);
                    self.invite_input = text;
                }
            }
        });
        ui.add_space(12.0);
        ui.label(RichText::new(t!("Depois de conectado, este computador só recebe conexões dos técnicos da empresa, sempre com o seu aceite na tela.")).size(11.5).color(p.muted));
    }
}
