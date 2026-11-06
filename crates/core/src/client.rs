//! Conexão de controle com o servidor: mantém o ID online, recebe convites e
//! pede conexões. Reconecta sozinha.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use elo_proto::ctrl::{FromServer, ToServer};
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
    let (ctx, crx) = channel::<Command>();
    let (etx, erx) = channel::<Event>();
    std::thread::Builder::new()
        .name("elo-controle".into())
        .spawn(move || run(cfg, token, name, crx, etx))
        .expect("thread");
    Control { tx: ctx, rx: erx }
}

fn run(mut cfg: ServerCfg, token: [u8; 32], name: String, rx: Receiver<Command>, tx: Sender<Event>) {
    let mut backoff = 1u64;
    loop {
        let _ = tx.send(Event::Connecting);
        match session(&cfg, &token, &name, &rx, &tx) {
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
                Ok(Command::Connect { .. }) => {
                    let _ = tx.send(Event::Error("sem conexão com o servidor".into()));
                }
                _ => {}
            }
        }
        backoff = (backoff * 2).min(30);
    }
}

/// Uma conexão. `Ok(Some(cfg))` = reconectar em outro servidor; `Ok(None)` = sair.
fn session(cfg: &ServerCfg, token: &[u8; 32], name: &str, rx: &Receiver<Command>, tx: &Sender<Event>) -> Result<Option<ServerCfg>, String> {
    let mut c = connect(cfg)?;
    let hello = ToServer::Hello { token: *token, name: name.to_string(), os: crate::platform::os_name(), version: elo_proto::PROTOCOL_VERSION, mac: crate::platform::mac_address() };
    write_frame(&mut c, &postcard::to_stdvec(&hello).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(Duration::from_millis(100)));
    let mut fr = Framer::new();
    let mut last_ping = Instant::now();
    let mut last_rx = Instant::now();
    let send = |c: &mut crate::net::Conn, m: &ToServer| -> Result<(), String> { write_frame(c, &postcard::to_stdvec(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string()) };
    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Command::Quit => return Ok(None),
                Command::Reconfigure(n) => return Ok(Some(n)),
                Command::Connect { target } => send(&mut c, &ToServer::Connect { target, from_name: name.to_string() })?,
                Command::Decline { session } => send(&mut c, &ToServer::Decline { session })?,
                Command::Wake { target } => send(&mut c, &ToServer::Wake { target })?,
            }
        }
        match fr.poll(&mut c) {
            Ok(Some(f)) => {
                last_rx = Instant::now();
                let Ok(m) = postcard::from_bytes::<FromServer>(&f) else { continue };
                let ev = match m {
                    FromServer::Welcome { id } => Event::Online { id },
                    FromServer::Incoming { session, secret, from_id, from_name } => Event::Incoming { session, secret, from_id, from_name },
                    FromServer::SessionReady { session, secret, target_name, target_os } => Event::SessionReady { session, secret, target_name, target_os },
                    FromServer::Declined { .. } => Event::Declined,
                    FromServer::Error { message, code } => {
                        if code == elo_proto::ctrl::ErrorCode::Version {
                            return Err(message);
                        }
                        Event::Error(message)
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
