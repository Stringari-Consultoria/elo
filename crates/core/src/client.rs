//! Conexão de controle com o servidor: mantém o ID online, recebe convites e
//! pede conexões. Reconecta sozinha.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use elo_proto::ctrl::{FromServer, Manage, Overview, ToServer};
use elo_proto::frame::write_frame;

use crate::net::{Framer, ServerCfg, connect};

#[derive(Debug, Clone)]
pub enum Event {
    Connecting,
    Online { id: String },
    Offline { reason: String },
    Incoming { session: u64, secret: [u8; 16], from_id: String, from_name: String },
    SessionReady { session: u64, secret: [u8; 16], target_name: String, target_os: String },
    Declined,
    Error(String),
    /// Pedido de Wake-on-LAN enviado (`relays`: computadores da rede do alvo que mandaram).
    WakeSent { target: String, relays: u32 },
    /// Recursos do servidor (logo depois de `Online`; vazio em servidores antigos).
    Features(Vec<String>),
    /// Elo Cliente: ligado à empresa. `ca`: chave pública dos técnicos (hex).
    Company { name: String, ca: String },
    /// Técnico: entrou (guarde o token).
    LoggedIn { user: String, name: String, admin: bool, token: String },
    /// O servidor ainda não tem técnicos: crie o administrador com o código de primeiro acesso.
    NeedsSetup,
    /// Usuário/senha errados, conta removida ou entrada expirada.
    AuthError(String),
    /// Elo Cliente: situação do pedido de ajuda (`None` = na fila).
    HelpStatus { taken_by: Option<String> },
    Overview(Box<Overview>),
}

/// Quem é este app para o servidor, além do ID: computador de uma empresa
/// (Elo Cliente) e/ou técnico logado. Vale também para as reconexões.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    /// Empresa e chave do convite.
    pub company: Option<(String, String)>,
    /// Token de uma entrada de técnico.
    pub tech_token: Option<String>,
    /// Chave pública deste app de técnico (hex), para a aprovação.
    pub tech_key: String,
}

#[derive(Debug, Clone)]
pub enum Command {
    Connect { target: String },
    Decline { session: u64 },
    /// Ligar um computador desligado (Wake-on-LAN).
    Wake { target: String },
    /// Troca de servidor.
    Reconfigure(ServerCfg),
    Quit,
    /// Técnico: entrar (com `setup`, cria o primeiro administrador).
    Login { user: String, password: String, setup: Option<String>, name: String },
    Logout,
    Manage(Manage),
    /// Elo Cliente: pedir ajuda / desistir.
    RequestHelp(String),
    CancelHelp,
    /// Troca a identidade (empresa do convite, token de técnico).
    SetIdentity(Identity),
}

pub struct Control {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
}

/// Testa um servidor: conecta, apresenta-se e devolve o ID que ele atribui a
/// este dispositivo (igual ao atual = é o mesmo servidor). Bloqueia até 15 s.
pub fn probe(cfg: &ServerCfg, token: &[u8; 32], name: &str) -> Result<String, String> {
    let mut c = connect(cfg)?;
    let hello = ToServer::Hello { token: *token, name: name.to_string(), os: crate::platform::os_name(), version: elo_proto::PROTOCOL_VERSION, mac: crate::platform::mac_address() };
    write_frame(&mut c, &postcard::to_stdvec(&hello).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(Duration::from_secs(10)));
    let f = elo_proto::frame::read_frame(&mut c).map_err(|e| format!("sem resposta do servidor: {e}"))?;
    match postcard::from_bytes::<FromServer>(&f) {
        Ok(FromServer::Welcome { id }) => Ok(id),
        Ok(FromServer::Error { message, .. }) => Err(message),
        _ => Err("resposta inesperada (não parece um servidor do Elo)".into()),
    }
}

/// Inicia a thread de controle.
pub fn start(cfg: ServerCfg, token: [u8; 32], name: String) -> Control {
    start_with(cfg, token, name, Identity::default())
}

/// Como `start`, já com a identidade (empresa e/ou técnico).
pub fn start_with(cfg: ServerCfg, token: [u8; 32], name: String, ident: Identity) -> Control {
    let (ctx, crx) = channel::<Command>();
    let (etx, erx) = channel::<Event>();
    std::thread::Builder::new()
        .name("elo-controle".into())
        .spawn(move || run(cfg, token, name, ident, crx, etx))
        .expect("thread");
    Control { tx: ctx, rx: erx }
}

fn run(mut cfg: ServerCfg, token: [u8; 32], name: String, mut ident: Identity, rx: Receiver<Command>, tx: Sender<Event>) {
    let mut backoff = 1u64;
    loop {
        let _ = tx.send(Event::Connecting);
        match session(&cfg, &token, &name, &mut ident, &rx, &tx) {
            Ok(Some(new)) => {
                cfg = new;
                backoff = 1;
                continue;
            }
            Ok(None) => return,
            Err(e) => {
                let _ = tx.send(Event::Offline { reason: e });
            }
        }
        // Espera, mas atende comandos (troca de servidor, sair).
        let until = Instant::now() + Duration::from_secs(backoff);
        while Instant::now() < until {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Command::Quit) => return,
                Ok(Command::Reconfigure(c)) => {
                    cfg = c;
                    break;
                }
                Ok(Command::Connect { .. }) | Ok(Command::Login { .. }) | Ok(Command::RequestHelp(_)) => {
                    let _ = tx.send(Event::Error("sem conexão com o servidor".into()));
                }
                Ok(Command::SetIdentity(i)) => ident = i,
                Ok(Command::Logout) => ident.tech_token = None,
                _ => {}
            }
        }
        backoff = (backoff * 2).min(30);
    }
}

/// Uma conexão. `Ok(Some(cfg))` = reconectar em outro servidor; `Ok(None)` = sair.
fn session(cfg: &ServerCfg, token: &[u8; 32], name: &str, ident: &mut Identity, rx: &Receiver<Command>, tx: &Sender<Event>) -> Result<Option<ServerCfg>, String> {
    let mut c = connect(cfg)?;
    let hello = ToServer::Hello { token: *token, name: name.to_string(), os: crate::platform::os_name(), version: elo_proto::PROTOCOL_VERSION, mac: crate::platform::mac_address() };
    write_frame(&mut c, &postcard::to_stdvec(&hello).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(Duration::from_millis(100)));
    let mut fr = Framer::new();
    let mut last_ping = Instant::now();
    let mut last_rx = Instant::now();
    let send = |c: &mut crate::net::Conn, m: &ToServer| -> Result<(), String> { write_frame(c, &postcard::to_stdvec(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string()) };
    // Servidor com "empresas"? (só então as mensagens novas podem ir).
    let mut enterprise = false;
    loop {
        while let Ok(cmd) = rx.try_recv() {
            let needs = |what: &str| -> Result<(), String> {
                let _ = tx.send(Event::Error(format!("este servidor do Elo é de uma versão antiga e não tem {what}: atualize-o")));
                Ok(())
            };
            match cmd {
                Command::Quit => return Ok(None),
                Command::Reconfigure(n) => return Ok(Some(n)),
                Command::Connect { target } => send(&mut c, &ToServer::Connect { target, from_name: name.to_string() })?,
                Command::Decline { session } => send(&mut c, &ToServer::Decline { session })?,
                Command::Wake { target } => send(&mut c, &ToServer::Wake { target })?,
                Command::SetIdentity(i) => {
                    if enterprise && let Some((company, key)) = i.company.clone().filter(|_| i.company != ident.company) {
                        send(&mut c, &ToServer::JoinCompany { company, key })?;
                    }
                    *ident = i;
                }
                Command::Login { user, password, setup, name } if enterprise => send(&mut c, &ToServer::Login { user, password, setup, name, key: ident.tech_key.clone() })?,
                Command::Login { .. } => needs("contas de técnico")?,
                Command::Logout => {
                    ident.tech_token = None;
                    if enterprise {
                        send(&mut c, &ToServer::Logout)?;
                    }
                }
                Command::Manage(m) if enterprise => send(&mut c, &ToServer::Manage(m))?,
                Command::RequestHelp(message) if enterprise => send(&mut c, &ToServer::RequestHelp { message })?,
                Command::CancelHelp if enterprise => send(&mut c, &ToServer::CancelHelp)?,
                Command::Manage(_) | Command::RequestHelp(_) | Command::CancelHelp => needs("empresas")?,
            }
        }
        match fr.poll(&mut c) {
            Ok(Some(f)) => {
                last_rx = Instant::now();
                let Ok((m, rest)) = postcard::take_from_bytes::<FromServer>(&f) else { continue };
                let ev = match m {
                    FromServer::Welcome { id } => {
                        let features: Vec<String> = if rest.is_empty() { Vec::new() } else { postcard::from_bytes(rest).unwrap_or_default() };
                        enterprise = features.iter().any(|f| f == "empresas");
                        let _ = tx.send(Event::Online { id });
                        let _ = tx.send(Event::Features(features));
                        if enterprise {
                            if let Some((company, key)) = ident.company.clone() {
                                send(&mut c, &ToServer::JoinCompany { company, key })?;
                            }
                            if let Some(token) = ident.tech_token.clone() {
                                send(&mut c, &ToServer::Resume { token, key: ident.tech_key.clone() })?;
                            }
                        }
                        continue;
                    }
                    FromServer::Company { name, ca } => Event::Company { name, ca },
                    FromServer::LoggedIn { user, name, admin, token } => {
                        ident.tech_token = Some(token.clone());
                        Event::LoggedIn { user, name, admin, token }
                    }
                    FromServer::NeedsSetup => Event::NeedsSetup,
                    FromServer::HelpStatus { taken_by } => Event::HelpStatus { taken_by },
                    FromServer::Overview(o) => Event::Overview(Box::new(o)),
                    FromServer::Incoming { session, secret, from_id, from_name } => Event::Incoming { session, secret, from_id, from_name: elo_proto::plain_text(&from_name, 80) },
                    FromServer::SessionReady { session, secret, target_name, target_os } => Event::SessionReady { session, secret, target_name: elo_proto::plain_text(&target_name, 64), target_os: elo_proto::plain_text(&target_os, 64) },
                    FromServer::Declined { .. } => Event::Declined,
                    FromServer::Error { message, code } => {
                        if code == elo_proto::ctrl::ErrorCode::Version {
                            return Err(message);
                        }
                        if code == elo_proto::ctrl::ErrorCode::Auth {
                            Event::AuthError(message)
                        } else {
                            Event::Error(message)
                        }
                    }
                    FromServer::WakeRequest { mac } => {
                        // Estamos na rede do computador desligado: manda o pacote mágico.
                        let _ = elo_proto::ctrl::wake_on_lan(&mac);
                        continue;
                    }
                    FromServer::WakeSent { target, relays } => Event::WakeSent { target, relays },
                    FromServer::Pong | FromServer::Paired => continue,
                };
                let _ = tx.send(ev);
            }
            Ok(None) => {}
            Err(e) => return Err(format!("conexão com o servidor caiu: {e}")),
        }
        if last_ping.elapsed() > Duration::from_secs(25) {
            send(&mut c, &ToServer::Ping)?;
            last_ping = Instant::now();
        }
        if last_rx.elapsed() > Duration::from_secs(70) {
            return Err("o servidor parou de responder".into());
        }
    }
}
