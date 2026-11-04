//! Estado do app: conexão com o servidor, convites, sessões como anfitrião e
//! como visualizador.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use elo_core::client::{self, Command, Control, Event};
use elo_core::session::{self, Handle, HostEvent, HostOptions, Remote, Transfer, ViewerEvent};
use elo_proto::msg::{Monitor, Permissions};

use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Online(String),
    Offline(String),
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct ChatLine {
    pub mine: bool,
    pub text: String,
}

/// Sessão em que este computador acessa outro.
pub struct ViewSession {
    pub target_id: String,
    pub target_name: String,
    pub os: String,
    pub handle: Handle,
    pub remote: Arc<Mutex<Remote>>,
    pub events: Receiver<ViewerEvent>,
    pub connected: bool,
    pub waiting: bool,
    pub sas: String,
    pub monitors: Vec<Monitor>,
    pub monitor: u8,
    pub permissions: Permissions,
    pub chat: Vec<ChatLine>,
    pub chat_open: bool,
    pub chat_input: String,
    pub transfers: Vec<Transfer>,
    pub texture: Option<egui::TextureHandle>,
    pub tex_version: u64,
    pub ended: Option<String>,
    pub fit: bool,
    pub clipboard: bool,
    pub quality: usize,
    pub mods: egui::Modifiers,
    pub started: Instant,
}

/// Sessão em que este computador está sendo acessado.
pub struct HostSession {
    pub from_id: String,
    pub from_name: String,
    pub handle: Handle,
    pub events: Receiver<HostEvent>,
    pub started: bool,
    pub sas: String,
    pub with_password: bool,
    pub permissions: Permissions,
    pub chat: Vec<ChatLine>,
    pub chat_input: String,
    pub transfers: Vec<Transfer>,
    pub ended: Option<String>,
    pub since: Instant,
}

pub struct Ask {
    pub from_id: String,
    pub from_name: String,
    pub sas: String,
    pub reply: Sender<Option<Permissions>>,
    pub permissions: Permissions,
    pub at: Instant,
}

pub struct EloApp {
    pub settings: Settings,
    pub status: Status,
    pub control: Control,
    pub temp_password: String,
    pub show_temp: bool,
    pub target: String,
    pub password: String,
    pub pending_connect: Option<(String, String)>,
    pub views: Vec<ViewSession>,
    pub active_tab: usize,
    pub hosts: Vec<HostSession>,
    pub asks: Vec<Ask>,
    pub toasts: Vec<Toast>,
    pub show_settings: bool,
    pub new_permanent: String,
}

impl EloApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let settings = Settings::load();
        stringari_ui::theme::install_fonts(&cc.egui_ctx);
        stringari_ui::theme::apply(&cc.egui_ctx, settings.dark);
        let control = client::start(settings.server.clone(), settings.token, settings.device_name.clone());
        Self {
            settings,
            status: Status::Connecting,
            control,
            temp_password: elo_core::temp_password(),
            show_temp: false,
            target: String::new(),
            password: String::new(),
            pending_connect: None,
            views: Vec::new(),
            active_tab: 0,
            hosts: Vec::new(),
            asks: Vec::new(),
            toasts: Vec::new(),
            show_settings: false,
            new_permanent: String::new(),
        }
    }

    pub fn toast(&mut self, t: impl Into<String>) {
        self.toasts.push(Toast { text: t.into(), error: false, at: Instant::now() });
    }

    pub fn error(&mut self, t: impl Into<String>) {
        self.toasts.push(Toast { text: t.into(), error: true, at: Instant::now() });
    }

    pub fn my_id(&self) -> Option<&str> {
        match &self.status {
            Status::Online(id) => Some(id),
            _ => None,
        }
    }

    pub fn connect(&mut self, target: String, password: String) {
        let t = elo_proto::ctrl::clean_id(&target);
        if t.len() != 9 {
            self.error("Digite um ID de 9 dígitos.");
            return;
        }
        if self.my_id() == Some(t.as_str()) {
            self.error("Esse é o ID deste computador.");
            return;
        }
        if !matches!(self.status, Status::Online(_)) {
            self.error("Sem conexão com o servidor do Elo.");
            return;
        }
        self.pending_connect = Some((t.clone(), password));
        let _ = self.control.tx.send(Command::Connect { target: t });
    }

    pub fn host_options(&self) -> HostOptions {
        let mut passwords = vec![self.temp_password.clone()];
        if self.settings.unattended && !self.settings.permanent_password.is_empty() {
            passwords.push(self.settings.permanent_password.clone());
        }
        HostOptions { passwords, name: self.settings.device_name.clone(), downloads: self.settings.downloads.clone(), default_permissions: self.settings.default_permissions }
    }

    pub fn reconnect_server(&mut self) {
        let _ = self.control.tx.send(Command::Reconfigure(self.settings.server.clone()));
    }

    pub fn downloads(&self) -> PathBuf {
        self.settings.downloads.clone()
    }

    /// Processa eventos de rede (chamado a cada quadro).
    pub fn pump(&mut self, ctx: &egui::Context) {
        let evs: Vec<Event> = self.control.rx.try_iter().collect();
        for e in evs {
            match e {
                Event::Connecting => {
                    if !matches!(self.status, Status::Online(_)) {
                        self.status = Status::Connecting;
                    }
                }
                Event::Online { id } => self.status = Status::Online(id),
                Event::Offline { reason } => self.status = Status::Offline(reason),
                Event::Error(m) => {
                    self.pending_connect = None;
                    self.error(m);
                }
                Event::Declined => {
                    self.pending_connect = None;
                    self.error("O outro computador recusou a conexão.");
                }
                Event::SessionReady { session, secret, target_name, target_os } => {
                    let Some((target_id, password)) = self.pending_connect.take() else { continue };
                    let remote = Arc::new(Mutex::new(Remote::default()));
                    let (tx, rx) = channel();
                    let my = self.my_id().unwrap_or_default().to_string();
                    let handle = session::viewer(self.settings.server.clone(), session, secret, password, my, self.settings.device_name.clone(), self.downloads(), remote.clone(), tx);
                    let q = self.settings.quality;
                    let (jpeg, fps) = quality(q);
                    let _ = handle.cmd.send(session::Cmd::Quality { jpeg, fps });
                    self.settings.remember(&target_id, &target_name, &target_os);
                    self.settings.save();
                    self.views.push(ViewSession {
                        target_id,
                        target_name,
                        os: target_os,
                        handle,
                        remote,
                        events: rx,
                        connected: false,
                        waiting: false,
                        sas: String::new(),
                        monitors: Vec::new(),
                        monitor: 0,
                        permissions: Permissions::default(),
                        chat: Vec::new(),
                        chat_open: false,
                        chat_input: String::new(),
                        transfers: Vec::new(),
                        texture: None,
                        tex_version: 0,
                        ended: None,
                        fit: true,
                        clipboard: true,
                        quality: q,
                        mods: egui::Modifiers::NONE,
                        started: Instant::now(),
                    });
                    self.active_tab = self.views.len();
                    self.password.clear();
                }
                Event::Incoming { session, secret, from_id, from_name } => {
                    if elo_core::session::host_locked() {
                        let _ = self.control.tx.send(Command::Decline { session });
                        self.error(format!("Conexão de {} bloqueada: muitas senhas erradas nos últimos minutos.", elo_proto::ctrl::pretty_id(&from_id)));
                        continue;
                    }
                    let (tx, rx) = channel();
                    let handle = session::host(self.settings.server.clone(), session, secret, from_id.clone(), from_name.clone(), self.host_options(), tx);
                    self.hosts.push(HostSession { from_id, from_name, handle, events: rx, started: false, sas: String::new(), with_password: false, permissions: self.settings.default_permissions, chat: Vec::new(), chat_input: String::new(), transfers: Vec::new(), ended: None, since: Instant::now() });
                }
            }
        }
        // Sessões como anfitrião.
        let mut new_asks = Vec::new();
        let mut toasts = Vec::new();
        for h in self.hosts.iter_mut() {
            for e in h.events.try_iter() {
                match e {
                    HostEvent::Ask { from_id, from_name, sas, reply } => new_asks.push(Ask { from_id, from_name, sas, reply, permissions: h.permissions, at: Instant::now() }),
                    HostEvent::Started { sas, with_password, .. } => {
                        h.started = true;
                        h.sas = sas;
                        h.with_password = with_password;
                        h.since = Instant::now();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    HostEvent::Chat(t) => h.chat.push(ChatLine { mine: false, text: t }),
                    HostEvent::File(t) => {
                        if t.finished && t.incoming {
                            toasts.push((format!("Arquivo recebido: {}", t.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()), false));
                        }
                        upsert(&mut h.transfers, t);
                    }
                    HostEvent::AuthFailed { from_id } => toasts.push((format!("Tentativa de acesso com senha errada de {}", elo_proto::ctrl::pretty_id(&from_id)), true)),
                    HostEvent::Ended(r) => h.ended = Some(r),
                }
            }
        }
        self.asks.extend(new_asks);
        let ended: Vec<String> = self.hosts.iter().filter(|h| h.ended.is_some() && h.started).map(|h| format!("{} encerrou: {}", h.from_name, h.ended.clone().unwrap_or_default())).collect();
        self.hosts.retain(|h| h.ended.is_none());
        for (t, e) in toasts {
            if e { self.error(t) } else { self.toast(t) }
        }
        for t in ended {
            self.toast(t);
        }
        // Sessões como visualizador.
        for v in self.views.iter_mut() {
            for e in v.events.try_iter() {
                match e {
                    ViewerEvent::Connected { name, os, sas, monitors, permissions } => {
                        v.connected = true;
                        v.waiting = false;
                        v.target_name = name;
                        v.os = os;
                        v.sas = sas;
                        v.monitor = monitors.iter().position(|m| m.primary).unwrap_or(0) as u8;
                        if v.monitor != 0 {
                            let _ = v.handle.cmd.send(session::Cmd::Monitor(v.monitor));
                        }
                        v.monitors = monitors;
                        v.permissions = permissions;
                    }
                    ViewerEvent::Waiting => v.waiting = true,
                    ViewerEvent::Chat(t) => {
                        v.chat.push(ChatLine { mine: false, text: t });
                        v.chat_open = true;
                    }
                    ViewerEvent::File(t) => upsert(&mut v.transfers, t),
                    ViewerEvent::Permissions(p) => v.permissions = p,
                    ViewerEvent::Ended(r) => v.ended = Some(r),
                }
            }
        }
        // Avisos expiram.
        self.toasts.retain(|t| t.at.elapsed().as_secs() < if t.error { 10 } else { 5 });
        let busy = !self.views.is_empty() || !self.hosts.is_empty() || !self.asks.is_empty() || matches!(self.status, Status::Connecting) || self.pending_connect.is_some();
        ctx.request_repaint_after(std::time::Duration::from_millis(if busy { 16 } else { 250 }));
    }
}

fn upsert(list: &mut Vec<Transfer>, t: Transfer) {
    match list.iter_mut().find(|x| x.id == t.id && x.incoming == t.incoming) {
        Some(x) => *x = t,
        None => list.push(t),
    }
}

/// (qualidade JPEG, fps) por perfil.
pub fn quality(q: usize) -> (u8, u8) {
    match q {
        0 => (35, 24),
        2 => (85, 12),
        _ => (60, 18),
    }
}
