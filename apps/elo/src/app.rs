//! Estado do app: conexão com o servidor, convites, sessões como anfitrião e
//! como visualizador.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use elo_core::client::{self, Command, Control, Event};
use elo_core::net::ServerCfg;
use elo_core::session::{self, Handle, HostEvent, HostOptions, Listing, Remote, Transfer, ViewerEvent};
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
        let c = elo_server::Config { listen: format!("{host}:{}", cfg.port), data: data.clone(), tls: elo_server::Tls::SelfSigned, retention_days: cfg.retention_days };
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

/// Reconexão automática a um computador que está reiniciando.
pub struct Reconnect {
    pub id: String,
    pub name: String,
    pub password: String,
    pub next_try: Instant,
    pub until: Instant,
}

/// Gerenciador de arquivos de uma sessão: pastas do computador acessado.
#[derive(Default)]
pub struct FilesPanel {
    pub open: bool,
    pub listing: Option<Listing>,
    /// Caminho na barra (editável).
    pub path: String,
    /// Nomes selecionados na pasta atual.
    pub selected: Vec<String>,
    pub hidden: bool,
    pub loading: bool,
}

/// Sessão em que este computador acessa outro.
pub struct ViewSession {
    /// Senha usada (para reconectar depois de reiniciar o computador remoto).
    pub password: String,
    /// Confirmação do "Reiniciar" aberta.
    pub confirm_restart: bool,
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
    /// Gerenciador de arquivos (painel lateral).
    pub files: FilesPanel,
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
    pub updater: crate::update::Updater,
    /// Aberto pelo início automático (reabre assim depois de atualizar).
    pub background: bool,
    /// Ícone na bandeja (se o sistema tiver).
    pub tray: Option<crate::tray::Tray>,
    /// "Sair do Elo": fechar a janela encerra de verdade.
    pub quitting: bool,
    /// Computadores reiniciando: reconecta quando voltarem.
    pub reconnects: Vec<Reconnect>,
    /// Painel do servidor aberto, com a última leitura.
    pub panel: Option<(Instant, elo_server::Stats)>,
    /// Janela escondida na bandeja.
    pub hidden: bool,
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
            updater: crate::update::Updater::default(),
            background,
            tray: crate::tray::create(&cc.egui_ctx),
            quitting: false,
            hidden: false,
            reconnects: Vec::new(),
            panel: None,
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
        HostOptions { passwords, name: self.settings.device_name.clone(), downloads: self.settings.downloads.clone(), default_permissions: self.settings.default_permissions, restart: elo_core::platform::restart_computer }
    }

    /// Servidor em uso: o local (se ligado) ou o configurado.
    pub fn server_cfg(&self) -> ServerCfg {
        server_cfg(&self.settings, &self.local)
    }

    /// Aplica as configurações de servidor: reinicia o local se mudou e
    /// reconecta o cliente.
    /// Arquivos arrastados para a janela durante uma sessão: envia para o
    /// outro computador (para a pasta aberta no gerenciador, se houver).
    pub fn drop_files(&mut self, ctx: &egui::Context, idx: usize, area: egui::Rect) {
        let Some(v) = self.views.get_mut(idx) else { return };
        let (hovering, dropped) = ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        if hovering && v.connected {
            let p = if self.settings.dark { stringari_ui::theme::DARK } else { stringari_ui::theme::LIGHT };
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("soltar-arquivos")));
            painter.rect_filled(area, 0.0, egui::Color32::from_black_alpha(150));
            let inner = area.shrink(24.0);
            painter.rect_stroke(inner, egui::CornerRadius::same(16), egui::Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
            let who = if v.target_name.is_empty() { elo_proto::ctrl::pretty_id(&v.target_id) } else { v.target_name.clone() };
            let text = if !v.permissions.files {
                "O outro lado não liberou a transferência de arquivos".to_string()
            } else if let Some(l) = v.files.listing.as_ref().filter(|_| v.files.open) {
                format!("Solte para enviar para {who}\n{}", l.path)
            } else {
                format!("Solte para enviar para {who}")
            };
            painter.text(inner.center(), egui::Align2::CENTER_CENTER, text, stringari_ui::theme::semibold(18.0), egui::Color32::WHITE);
        }
        if dropped.is_empty() || !v.connected {
            return;
        }
        if !v.permissions.files {
            self.error("O outro lado não liberou a transferência de arquivos.");
            return;
        }
        let dir = v.files.listing.as_ref().filter(|_| v.files.open).map(|l| l.path.clone());
        let mut n = 0;
        for f in dropped {
            let path = f.path().to_path_buf();
            if path.as_os_str().is_empty() {
                continue;
            }
            let c = match &dir {
                Some(d) => session::Cmd::Upload(path, d.clone()),
                None => session::Cmd::SendFile(path),
            };
            let _ = v.handle.cmd.send(c);
            n += 1;
        }
        if n > 0 && !v.files.open {
            // Mostra o progresso.
            v.chat_open = true;
        }
    }

    /// Instala a atualização baixada e sai (o Elo novo abre sozinho).
    pub fn update_and_restart(&mut self) {
        match self.updater.install_now(self.background) {
            Ok(()) => {
                for v in &self.views {
                    v.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                for h in &self.hosts {
                    h.handle.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.settings.save();
                self.local.server = None;
                std::process::exit(0);
            }
            Err(e) => self.error(format!("Atualização: {e}")),
        }
    }

    /// Traz a janela de volta (da bandeja ou minimizada) para a frente.
    pub fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Eventos do ícone da bandeja.
    pub fn tray_tick(&mut self, ctx: &egui::Context) {
        let id = self.my_id().map(str::to_string);
        let Some(t) = self.tray.as_mut() else { return };
        t.set_id(id.as_deref());
        let acts: Vec<_> = t.rx.try_iter().collect();
        for a in acts {
            match a {
                crate::tray::Act::Open => self.show_window(ctx),
                crate::tray::Act::CopyId => {
                    if let Some(id) = &id {
                        ctx.copy_text(elo_proto::ctrl::pretty_id(id));
                    }
                }
                crate::tray::Act::Quit => {
                    self.quitting = true;
                    // A janela precisa estar visível para receber o pedido de fechar.
                    self.show_window(ctx);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

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
        if self.local.started.retention_days != self.settings.local_server.retention_days {
            self.local.started.retention_days = self.settings.local_server.retention_days;
            if let Some(s) = &self.local.server {
                s.set_retention_days(self.settings.local_server.retention_days);
            }
        }
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
                    // Tentativa de reconexão: o computador ainda está voltando, sem alarde.
                    let retry = self.pending_connect.as_ref().is_some_and(|(t, _)| self.reconnects.iter().any(|r| &r.id == t));
                    self.pending_connect = None;
                    if !retry {
                        self.error(m);
                    }
                }
                Event::WakeSent { target, relays } => {
                    let via = if relays == 0 { "pelo servidor".to_string() } else { format!("pelo servidor e por {relays} computador(es) da rede dele") };
                    self.toast(format!("Pedido para ligar {} enviado {via}. Pode levar um minuto; o computador precisa ter o Wake-on-LAN ligado.", elo_proto::ctrl::pretty_id(&target)));
                }
                Event::Declined => {
                    self.pending_connect = None;
                    self.error("O outro computador recusou a conexão.");
                }
                Event::SessionReady { session, secret, target_name, target_os } => {
                    let Some((target_id, password)) = self.pending_connect.take() else { continue };
                    // Reconexão que deu certo sai da fila.
                    self.reconnects.retain(|r| r.id != target_id);
                    let session_password = password.clone();
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
                        password: session_password,
                        confirm_restart: false,
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
                        files: FilesPanel::default(),
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
        let mut show = false;
        let mut new_asks = Vec::new();
        let mut toasts = Vec::new();
        for h in self.hosts.iter_mut() {
            for e in h.events.try_iter() {
                match e {
                    HostEvent::Ask { from_id, from_name, sas, reply } => {
                        new_asks.push(Ask { from_id, from_name, sas, reply, permissions: h.permissions, at: Instant::now() });
                        show = true;
                    }
                    HostEvent::Started { sas, with_password, .. } => {
                        h.started = true;
                        h.sas = sas;
                        h.with_password = with_password;
                        h.since = Instant::now();
                        // Quem está aqui precisa ver o aviso de que está sendo acessado.
                        show = true;
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
        if show {
            self.show_window(ctx);
        }
        let ended: Vec<String> = self.hosts.iter().filter(|h| h.ended.is_some() && h.started).map(|h| format!("{} encerrou: {}", h.from_name, h.ended.clone().unwrap_or_default())).collect();
        self.hosts.retain(|h| h.ended.is_none());
        for (t, e) in toasts {
            if e { self.error(t) } else { self.toast(t) }
        }
        for t in ended {
            self.toast(t);
        }
        // Sessões como visualizador.
        let mut notices = Vec::new();
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
                    ViewerEvent::Permissions(p) => {
                        v.permissions = p;
                        if !p.files {
                            v.files.open = false;
                        }
                    }
                    ViewerEvent::Listing(l) => {
                        v.files.path = l.path.clone();
                        v.files.selected.clear();
                        v.files.loading = false;
                        v.files.listing = Some(l);
                    }
                    ViewerEvent::Notice(t) => notices.push(t),
                    ViewerEvent::Ended(r) => v.ended = Some(r),
                }
            }
        }
        for t in notices {
            self.error(t);
        }
        // Reconexões: tenta de 10 em 10 s enquanto o computador reinicia.
        let now = Instant::now();
        let expired: Vec<String> = self.reconnects.iter().filter(|r| now > r.until).map(|r| r.name.clone()).collect();
        self.reconnects.retain(|r| now <= r.until);
        for n in expired {
            self.error(format!("{n} não voltou depois de reiniciar. Confira se ele inicia o Elo com o computador."));
        }
        if matches!(self.status, Status::Online(_)) && self.pending_connect.is_none()
            && let Some(r) = self.reconnects.iter_mut().find(|r| now >= r.next_try)
        {
            r.next_try = now + std::time::Duration::from_secs(10);
            let (id, pw) = (r.id.clone(), r.password.clone());
            self.pending_connect = Some((id.clone(), pw));
            let _ = self.control.tx.send(Command::Connect { target: id });
        }
        if !self.reconnects.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // Atualizações: instala sozinho só sem sessão nem pedido em andamento.
        let idle = self.views.is_empty() && self.hosts.is_empty() && self.asks.is_empty() && self.pending_connect.is_none();
        if self.updater.tick(self.settings.auto_update, idle) {
            self.update_and_restart();
        }
        if self.updater.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
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
