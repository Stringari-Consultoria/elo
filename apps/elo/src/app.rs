//! Estado do app: conexão com o servidor, convites, sessões como anfitrião e
//! como visualizador.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use elo_core::client::{self, Command, Control, Event};
use elo_core::net::ServerCfg;
use elo_core::session::{self, Handle, HostEvent, HostOptions, Remote, Transfer, ViewerEvent};
use elo_proto::msg::{Monitor, Permissions};

use crate::settings::{LocalServer, Settings};

/// Servidor embutido no app.
pub struct Local {
    /// Configuração com que foi iniciado (para saber se precisa reiniciar).
    pub started: LocalServer,
    pub server: Option<elo_server::Background>,
    pub error: Option<String>,
    /// Impressão digital do certificado (os outros computadores colam nas configurações).
    pub fingerprint: String,
    /// IP deste computador na rede, para os outros conectarem.
    pub lan_ip: Option<String>,
}

impl Local {
    fn start(cfg: &LocalServer) -> Self {
        let mut l = Local { started: cfg.clone(), server: None, error: None, fingerprint: String::new(), lan_ip: None };
        if !cfg.enabled {
            return l;
        }
        let data = crate::settings::server_data_dir();
        let host = if cfg.lan { "0.0.0.0" } else { "127.0.0.1" };
        let c = elo_server::Config { listen: format!("{host}:{}", cfg.port), data: data.clone(), tls: elo_server::Tls::SelfSigned };
        match elo_server::Background::start(c) {
            Ok(b) => {
                l.fingerprint = b.fingerprint.clone().unwrap_or_default();
                l.server = Some(b);
            }
            Err(e) => {
                // Porta ocupada por outra janela do Elo com a mesma pasta de
                // configuração: o certificado é o mesmo, então ainda conecta nela.
                l.fingerprint = elo_server::self_signed_fingerprint(&data).unwrap_or_default();
                l.error = Some(e.to_string());
            }
        }
        l.lan_ip = lan_ip();
        l
    }

    /// Endereço da rede local ("ip:porta").
    pub fn lan_address(&self) -> String {
        format!("{}:{}", self.lan_ip.as_deref().unwrap_or("IP-deste-computador"), self.started.port)
    }
}

/// IP local da interface de saída (não envia nada: UDP só escolhe a rota).
fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

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
    /// Último movimento do mouse local sobre a tela remota.
    pub last_move: Instant,
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
    pub local: Local,
    /// Teste de servidor em andamento; `true` = tem de ser este mesmo servidor.
    pub probe: Option<(Receiver<Result<String, String>>, bool)>,
    /// Resultado do último teste: (deu certo, mensagem).
    pub probe_result: Option<(bool, String)>,
    /// Boas-vindas (primeira abertura).
    pub onboarding: Option<crate::onboarding::Onboarding>,
    /// Iniciar com o computador (como está no sistema).
    pub autostart: bool,
    /// Aberto pelo início automático: minimiza no primeiro quadro.
    pub start_minimized: bool,
    /// Sessão em tela cheia (esconde as barras do Elo).
    pub fullscreen: bool,
    /// Favorito sendo renomeado: (ID, texto).
    pub renaming: Option<(String, String)>,
    /// Ação pedida na barra da sessão (aplicada depois de desenhar).
    pub session_act: Option<crate::ui::SessAct>,
}

impl EloApp {
    pub fn new(cc: &eframe::CreationContext, background: bool) -> Self {
        let settings = Settings::load();
        crate::autostart::refresh();
        stringari_ui::theme::install_fonts(&cc.egui_ctx);
        stringari_ui::theme::apply(&cc.egui_ctx, settings.dark);
        // O servidor local sobe antes do cliente, que já conecta nele.
        let local = Local::start(&settings.local_server);
        let control = client::start(server_cfg(&settings, &local), settings.token, settings.device_name.clone());
        let local_error = local.error.clone();
        let mut app = Self {
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
            local,
            probe: None,
            probe_result: None,
            onboarding: None,
            autostart: crate::autostart::enabled(),
            start_minimized: background,
            fullscreen: false,
            renaming: None,
            session_act: None,
        };
        if !app.settings.onboarded {
            app.onboarding = Some(crate::onboarding::Onboarding::new(&app.settings, app.autostart));
        }
        if let Some(e) = local_error {
            app.error(format!("Servidor local: {e}"));
        }
        app
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

    /// Servidor em uso: o local (se ligado) ou o configurado.
    pub fn server_cfg(&self) -> ServerCfg {
        server_cfg(&self.settings, &self.local)
    }

    /// Aplica as configurações de servidor: reinicia o local se mudou e
    /// reconecta o cliente.
    pub fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    /// Testa o endereço público (wss://) numa thread: tem de chegar a este servidor.
    pub fn test_public(&mut self) {
        let Some(addr) = self.settings.local_server.public_ws().map(str::to_string) else { return };
        self.test_server(ServerCfg { address: addr, tls: true, pin: String::new() }, true);
    }

    /// Testa um servidor numa thread. `mine`: tem de ser este mesmo servidor
    /// (endereço público); senão, basta responder (servidor de outra pessoa).
    pub fn test_server(&mut self, cfg: ServerCfg, mine: bool) {
        let (token, name) = (self.settings.token, self.settings.device_name.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(client::probe(&cfg, &token, &name));
        });
        self.probe = Some((rx, mine));
        self.probe_result = None;
    }

    /// Recolhe o resultado do teste, se já chegou.
    pub fn poll_probe(&mut self) {
        let Some((r, same)) = self.probe.as_ref().and_then(|(rx, same)| rx.try_recv().ok().map(|r| (r, *same))) else { return };
        self.probe = None;
        let mine = self.my_id().map(str::to_string);
        self.probe_result = Some(match r {
            Ok(id) if !same => (true, format!("Conectado. Neste servidor, este computador terá o ID {}.", elo_proto::ctrl::pretty_id(&id))),
            Ok(id) if Some(&id) == mine.as_ref() => (true, "Funcionando: o endereço público chega a este servidor.".into()),
            Ok(_) => (false, "O endereço responde, mas é outro servidor do Elo (o ID deste computador lá é diferente).".into()),
            Err(e) if e.contains("502") || e.contains("530") => (false, format!("O Cloudflare não alcançou o Elo ({e}). Confira o Service da rota, nas instruções abaixo.")),
            Err(e) => (false, format!("Não conectou: {e}")),
        });
    }

    /// Reinicia a conexão com o servidor (ex.: o nome do computador mudou,
    /// e ele só é enviado ao conectar).
    pub fn restart_control(&mut self) {
        let _ = self.control.tx.send(Command::Quit);
        self.control = client::start(self.server_cfg(), self.settings.token, self.settings.device_name.clone());
        self.status = Status::Connecting;
    }

    pub fn reconnect_server(&mut self) {
        if !self.settings.local_server.same_bind(&self.local.started) {
            // Libera a porta antes de subir de novo.
            self.local.server = None;
            self.local = Local::start(&self.settings.local_server);
            if let Some(e) = &self.local.error {
                let e = format!("Servidor local: {e}");
                self.error(e);
            }
        }
        self.local.started.public = self.settings.local_server.public.clone();
        let _ = self.control.tx.send(Command::Reconfigure(self.server_cfg()));
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
                    let handle = session::viewer(self.server_cfg(), session, secret, password, my, self.settings.device_name.clone(), self.downloads(), remote.clone(), tx);
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
                        last_move: Instant::now(),
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
                    let handle = session::host(self.server_cfg(), session, secret, from_id.clone(), from_name.clone(), self.host_options(), tx);
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

fn server_cfg(s: &Settings, local: &Local) -> ServerCfg {
    if s.local_server.enabled {
        ServerCfg { address: format!("127.0.0.1:{}", s.local_server.port), tls: true, pin: local.fingerprint.clone() }
    } else {
        s.server.clone()
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
