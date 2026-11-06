//! Sessões: anfitrião (quem é acessado) e visualizador (quem acessa).

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elo_proto::codec::{Decoder, Encoder};
use elo_proto::ctrl::{FromServer, ToServer};
use elo_proto::frame::{read_frame, write_frame};
use elo_proto::msg::{DirEntry, Input, Monitor, Msg, Permissions};
use elo_proto::secure::{AuthMode, Opener, Sealer, host_handshake, viewer_handshake};

use crate::net::{Conn, Framer, ServerCfg, connect};
use crate::platform::{self, Clip};

const CHUNK: usize = 192 * 1024;

/// Canal cifrado com leitura não bloqueante.
struct Chan {
    c: Conn,
    fr: Framer,
    seal: Sealer,
    open: Opener,
    pub sent: u64,
    pub recv: u64,
}

impl Chan {
    fn send(&mut self, m: &Msg) -> Result<(), String> {
        let data = self.seal.seal(&postcard::to_stdvec(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        self.sent += data.len() as u64 + 4;
        write_frame(&mut self.c, &data).map_err(|e| e.to_string())
    }

    fn poll(&mut self) -> Result<Option<Msg>, String> {
        match self.fr.poll(&mut self.c) {
            Ok(Some(f)) => {
                self.recv += f.len() as u64 + 4;
                let plain = self.open.open(&f).map_err(|e| e.to_string())?;
                postcard::from_bytes(&plain).map(Some).map_err(|e| e.to_string())
            }
            Ok(None) => Ok(None),
            Err(e) => Err(if e.kind() == std::io::ErrorKind::UnexpectedEof { "o outro lado encerrou".into() } else { e.to_string() }),
        }
    }
}

/// Entra no relay e espera o par.
fn join(cfg: &ServerCfg, session: u64, secret: [u8; 16], host: bool) -> Result<Conn, String> {
    let mut c = connect(cfg)?;
    write_frame(&mut c, &postcard::to_stdvec(&ToServer::Join { session, secret, host }).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(Duration::from_secs(125)));
    let f = read_frame(&mut c).map_err(|e| format!("o outro lado não entrou: {e}"))?;
    match postcard::from_bytes::<FromServer>(&f).map_err(|e| e.to_string())? {
        FromServer::Paired => Ok(c),
        FromServer::Error { message, .. } => Err(message),
        _ => Err("resposta inesperada do servidor".into()),
    }
}

/// Transferência de arquivos (nos dois sentidos).
#[derive(Default)]
struct Files {
    outgoing: Vec<(u32, File, u64, u64, String)>,
    incoming: Vec<(u32, File, PathBuf, u64, u64)>,
    next: u32,
    offered: Vec<(u32, PathBuf, u64)>,
}

#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: u32,
    pub name: String,
    pub done: u64,
    pub total: u64,
    pub incoming: bool,
    pub finished: bool,
    pub path: Option<PathBuf>,
}

impl Files {
    fn offer(&mut self, ch: &mut Chan, path: &Path, dir: Option<String>) -> Result<Transfer, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            return Err("pastas ainda não podem ser enviadas; envie os arquivos".into());
        }
        let size = meta.len();
        self.next += 1;
        let id = self.next;
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "arquivo".into());
        ch.send(&Msg::FileOffer { id, name: name.clone(), size, dir })?;
        self.offered.push((id, path.to_path_buf(), size));
        Ok(Transfer { id, name, done: 0, total: size, incoming: false, finished: false, path: Some(path.to_path_buf()) })
    }

    fn accepted(&mut self, id: u32) -> Result<(), String> {
        let Some(k) = self.offered.iter().position(|x| x.0 == id) else { return Ok(()) };
        let (id, path, size) = self.offered.remove(k);
        let f = File::open(&path).map_err(|e| e.to_string())?;
        self.outgoing.push((id, f, 0, size, path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        Ok(())
    }

    /// Envia um pedaço de cada envio em andamento. Devolve progresso.
    fn pump(&mut self, ch: &mut Chan, out: &mut Vec<Transfer>) -> Result<(), String> {
        let mut done = Vec::new();
        for (id, f, sent, total, name) in self.outgoing.iter_mut() {
            let mut buf = vec![0u8; CHUNK];
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                ch.send(&Msg::FileEnd { id: *id })?;
                done.push(*id);
                out.push(Transfer { id: *id, name: name.clone(), done: *sent, total: *total, incoming: false, finished: true, path: None });
                continue;
            }
            buf.truncate(n);
            ch.send(&Msg::FileChunk { id: *id, data: buf })?;
            *sent += n as u64;
            out.push(Transfer { id: *id, name: name.clone(), done: *sent, total: *total, incoming: false, finished: false, path: None });
        }
        self.outgoing.retain(|x| !done.contains(&x.0));
        Ok(())
    }

    fn start_incoming(&mut self, dir: &Path, id: u32, name: &str, size: u64) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        // Nome seguro (sem caminhos) e sem sobrescrever.
        let clean: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0')).collect();
        let clean = if clean.trim_matches('.').is_empty() { "arquivo".to_string() } else { clean };
        let mut p = dir.join(&clean);
        let mut k = 1;
        while p.exists() {
            let stem = Path::new(&clean).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = Path::new(&clean).extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
            p = dir.join(format!("{stem} ({k}){ext}"));
            k += 1;
        }
        let f = File::create(&p).map_err(|e| e.to_string())?;
        self.incoming.push((id, f, p.clone(), 0, size));
        Ok(p)
    }

    fn chunk(&mut self, id: u32, data: &[u8]) -> Option<Transfer> {
        let x = self.incoming.iter_mut().find(|x| x.0 == id)?;
        if x.1.write_all(data).is_err() {
            return None;
        }
        x.3 += data.len() as u64;
        Some(Transfer { id, name: x.2.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), done: x.3, total: x.4, incoming: true, finished: false, path: Some(x.2.clone()) })
    }

    fn end(&mut self, id: u32) -> Option<Transfer> {
        let k = self.incoming.iter().position(|x| x.0 == id)?;
        let (id, f, p, done, total) = self.incoming.remove(k);
        drop(f);
        Some(Transfer { id, name: p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), done, total, incoming: true, finished: true, path: Some(p) })
    }
}

/// Pasta pessoal do usuário.
fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// Lista uma pasta do anfitrião para o gerenciador de arquivos.
fn list_dir(path: &str) -> Msg {
    const MAX: usize = 10_000;
    let p = if path.trim().is_empty() { home_dir() } else { PathBuf::from(path.trim()) };
    let p = std::fs::canonicalize(&p).unwrap_or(p);
    // No Windows, canonicalize devolve "\\?\C:\..."; mostra o caminho comum.
    let shown = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let parent = Path::new(&shown).parent().map(|x| x.to_string_lossy().into_owned());
    let rd = match std::fs::read_dir(&p) {
        Ok(rd) => rd,
        Err(e) => return Msg::DirListing { path: shown, parent, entries: Vec::new(), error: Some(e.to_string()) },
    };
    let mut entries: Vec<DirEntry> = rd
        .filter_map(|e| e.ok())
        .take(MAX)
        .map(|e| {
            // Segue links simbólicos (pasta que aponta para pasta).
            let meta = std::fs::metadata(e.path()).or_else(|_| e.metadata());
            let (dir, size, modified) = match meta {
                Ok(m) => (m.is_dir(), if m.is_dir() { 0 } else { m.len() }, m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)),
                Err(_) => (false, 0, 0),
            };
            DirEntry { name: e.file_name().to_string_lossy().into_owned(), dir, size, modified }
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Msg::DirListing { path: shown, parent, entries, error: None }
}

// ── anfitrião ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HostOptions {
    /// Senhas aceitas, na ordem (temporária, acesso não assistido).
    pub passwords: Vec<String>,
    pub name: String,
    pub downloads: PathBuf,
    pub default_permissions: Permissions,
    /// Como reiniciar o computador (testes trocam por uma função que não reinicia).
    pub restart: fn() -> Result<(), String>,
}

/// Motivo de fim quando quem pediu acesso desistiu antes da resposta.
pub const GAVE_UP: &str = "quem pediu desistiu do pedido";

pub enum HostEvent {
    /// Pedido sem senha: responda com as permissões (ou `None` para recusar).
    Ask { from_id: String, from_name: String, sas: String, reply: Sender<Option<Permissions>> },
    Started { from_id: String, from_name: String, sas: String, with_password: bool },
    Chat(String),
    File(Transfer),
    AuthFailed { from_id: String },
    Ended(String),
}

/// Controle de uma sessão em andamento (dos dois lados).
#[derive(Clone)]
pub struct Handle {
    pub stop: Arc<AtomicBool>,
    pub cmd: Sender<Cmd>,
    pub bytes_sent: Arc<AtomicU64>,
    pub bytes_recv: Arc<AtomicU64>,
    pub fps: Arc<AtomicU32>,
}

pub enum Cmd {
    Input(Input),
    Chat(String),
    SendFile(PathBuf),
    /// Gerenciador de arquivos (visualizador): listar pasta do anfitrião,
    /// baixar um arquivo de lá e enviar um arquivo para uma pasta de lá.
    ListDir(String),
    Download(String),
    Upload(PathBuf, String),
    /// Reiniciar o computador do anfitrião.
    Restart,
    Quality { jpeg: u8, fps: u8 },
    Monitor(u8),
    Refresh,
    Permissions(Permissions),
    Clipboard(bool),
    SecureAttention,
}

fn new_handle() -> (Handle, Receiver<Cmd>) {
    let (tx, rx) = channel();
    (Handle { stop: Arc::new(AtomicBool::new(false)), cmd: tx, bytes_sent: Arc::new(AtomicU64::new(0)), bytes_recv: Arc::new(AtomicU64::new(0)), fps: Arc::new(AtomicU32::new(0)) }, rx)
}

/// Tentativas erradas recentes (bloqueio contra força bruta).
static FAILS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

pub fn host_locked() -> bool {
    let mut f = FAILS.lock().unwrap_or_else(|e| e.into_inner());
    f.retain(|t| t.elapsed() < Duration::from_secs(300));
    f.len() >= 5
}

fn note_fail() {
    FAILS.lock().unwrap_or_else(|e| e.into_inner()).push(Instant::now());
}

pub fn host(cfg: ServerCfg, session: u64, secret: [u8; 16], from_id: String, from_name: String, opt: HostOptions, events: Sender<HostEvent>) -> Handle {
    let (h, rx) = new_handle();
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("elo-anfitriao".into())
        .spawn(move || {
            let r = host_run(&cfg, session, secret, &from_id, &from_name, &opt, &events, &h2, rx);
            let _ = events.send(HostEvent::Ended(r.err().unwrap_or_else(|| "sessão encerrada".into())));
        })
        .expect("thread");
    h
}

fn host_run(cfg: &ServerCfg, session: u64, secret: [u8; 16], from_id: &str, from_name: &str, opt: &HostOptions, ev: &Sender<HostEvent>, h: &Handle, rx: Receiver<Cmd>) -> Result<(), String> {
    let mut c = join(cfg, session, secret, true)?;
    c.set_read_timeout(Some(Duration::from_secs(30)));
    let keys = match host_handshake(&mut c, &opt.passwords) {
        Ok(k) => k,
        Err(e) => {
            if matches!(e, elo_proto::Error::Auth) {
                note_fail();
                let _ = ev.send(HostEvent::AuthFailed { from_id: from_id.into() });
                return Err("senha incorreta".into());
            }
            return Err(e.to_string());
        }
    };
    let mut ch = Chan { c, fr: Framer::new(), seal: Sealer::new(&keys.send), open: Opener::new(&keys.recv), sent: 0, recv: 0 };
    let mut perms = opt.default_permissions;
    if keys.mode == AuthMode::Request {
        let (tx, rrx) = channel();
        let _ = ev.send(HostEvent::Ask { from_id: from_id.into(), from_name: from_name.into(), sas: keys.sas.clone(), reply: tx });
        // Espera a resposta de quem está aqui, de olho na conexão: se o outro
        // lado desistir (cancelar, fechar), o pedido some da tela.
        ch.c.set_read_timeout(Some(Duration::from_millis(200)));
        let start = Instant::now();
        let answer = loop {
            match rrx.try_recv() {
                Ok(a) => break a,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            if h.stop.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(90) {
                break None;
            }
            match ch.poll() {
                Ok(Some(Msg::Bye(_))) | Err(_) => return Err(GAVE_UP.into()),
                _ => {}
            }
        };
        match answer {
            Some(p) => perms = p,
            None => {
                let _ = ch.send(&Msg::Bye("o pedido de acesso foi recusado".into()));
                return Err("pedido recusado".into());
            }
        }
    }
    let mut screen = platform::open_screen()?;
    let mut control = platform::open_control().ok();
    let mut clip = Clip::new();
    let monitors = screen.monitors();
    ch.send(&Msg::Welcome { name: opt.name.clone(), os: platform::os_name(), monitors, permissions: perms })?;
    let _ = ev.send(HostEvent::Started { from_id: from_id.into(), from_name: from_name.into(), sas: keys.sas.clone(), with_password: keys.mode == AuthMode::Password });
    ch.c.set_read_timeout(Some(Duration::from_millis(4)));
    let mut enc = Encoder::new();
    let (mut jpeg_q, mut fps) = (60u8, 15u8);
    let mut monitor = 0u8;
    let mut seq = 0u32;
    let mut acked = 0u32;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let mut last_change = Instant::now();
    let mut refined = false;
    let mut full = true;
    let mut last_clip = Instant::now();
    let mut files = Files::default();
    let mut frames_win = (Instant::now(), 0u32);
    let mut last_rx = Instant::now();
    loop {
        if h.stop.load(Ordering::Relaxed) {
            let _ = ch.send(&Msg::Bye("o anfitrião encerrou a sessão".into()));
            return Ok(());
        }
        // Mensagens do visualizador.
        for _ in 0..64 {
            let Some(m) = ch.poll()? else { break };
            last_rx = Instant::now();
            match m {
                Msg::Hello { .. } => {}
                Msg::Ack { seq: s } => acked = acked.max(s),
                Msg::Input(i) => {
                    if perms.control
                        && let Some(ctl) = control.as_mut()
                    {
                        ctl.apply(&i, monitor);
                    }
                }
                Msg::Clipboard(t) => {
                    if perms.clipboard {
                        clip.set(&t);
                    }
                }
                Msg::Chat(t) => {
                    let _ = ev.send(HostEvent::Chat(t));
                }
                Msg::Quality { jpeg, fps: f } => {
                    jpeg_q = jpeg.clamp(10, 95);
                    fps = f.clamp(1, 60);
                    full = true;
                }
                Msg::SelectMonitor(i) => {
                    monitor = i;
                    full = true;
                }
                Msg::Refresh => full = true,
                Msg::FileOffer { id, name, size, dir } => {
                    if perms.files {
                        // Pasta escolhida no gerenciador (se existir); senão, a de recebidos.
                        let target = dir.map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(|| opt.downloads.clone());
                        match files.start_incoming(&target, id, &name, size) {
                            Ok(_) => ch.send(&Msg::FileAccept { id })?,
                            Err(_) => ch.send(&Msg::FileCancel { id })?,
                        }
                    } else {
                        ch.send(&Msg::FileCancel { id })?;
                    }
                }
                Msg::FileAccept { id } => files.accepted(id)?,
                Msg::FileChunk { id, data } => {
                    if let Some(t) = files.chunk(id, &data) {
                        let _ = ev.send(HostEvent::File(t));
                    }
                }
                Msg::FileEnd { id } => {
                    if let Some(t) = files.end(id) {
                        let _ = ev.send(HostEvent::File(t));
                    }
                }
                Msg::Restart => {
                    if perms.control {
                        let _ = ch.send(&Msg::Bye(elo_proto::msg::BYE_RESTART.into()));
                        let _ = ev.send(HostEvent::Chat(format!("[{} pediu para reiniciar este computador]", from_name)));
                        (opt.restart)()?;
                        return Err("reiniciando o computador a pedido de quem acessava".into());
                    }
                    ch.send(&Msg::Notice("sem permissão de controle para reiniciar".into()))?;
                }
                Msg::ListDir { path } => {
                    let reply = if perms.files { list_dir(&path) } else { Msg::DirListing { path, parent: None, entries: Vec::new(), error: Some("o outro lado não liberou a transferência de arquivos".into()) } };
                    ch.send(&reply)?;
                }
                Msg::FileRequest { path } => {
                    if !perms.files {
                        ch.send(&Msg::Notice("o outro lado não liberou a transferência de arquivos".into()))?;
                    } else {
                        match files.offer(&mut ch, Path::new(&path), None) {
                            Ok(t) => {
                                let _ = ev.send(HostEvent::File(t));
                            }
                            Err(e) => ch.send(&Msg::Notice(format!("não foi possível baixar {}: {e}", Path::new(&path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(path.clone()))))?,
                        }
                    }
                }
                Msg::Ping(n) => ch.send(&Msg::Pong(n))?,
                Msg::Bye(r) => return Err(r),
                _ => {}
            }
        }
        // Comandos da interface local.
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Chat(t) => ch.send(&Msg::Chat(t))?,
                Cmd::SendFile(p) => {
                    if let Ok(t) = files.offer(&mut ch, &p, None) {
                        let _ = ev.send(HostEvent::File(t));
                    }
                }
                Cmd::Permissions(p) => {
                    perms = p;
                    ch.send(&Msg::Permissions(p))?;
                }
                _ => {}
            }
        }
        let mut progress = Vec::new();
        files.pump(&mut ch, &mut progress)?;
        for t in progress {
            let _ = ev.send(HostEvent::File(t));
        }
        // Área de transferência local → visualizador.
        if perms.clipboard && last_clip.elapsed() > Duration::from_millis(600) {
            last_clip = Instant::now();
            if let Some(t) = clip.changed() {
                ch.send(&Msg::Clipboard(t))?;
            }
        }
        // Quadro: no máximo 2 sem confirmação e respeitando o fps.
        let interval = Duration::from_millis(1000 / fps.max(1) as u64);
        if seq.wrapping_sub(acked) < 2 && last_frame.elapsed() >= interval {
            last_frame = Instant::now();
            let f = screen.capture(monitor)?;
            let (mut tiles, was_full) = enc.encode(&f.rgba, f.width, f.height, jpeg_q, full);
            full = false;
            if !tiles.is_empty() {
                last_change = Instant::now();
                refined = false;
            } else if !refined && last_change.elapsed() > Duration::from_millis(700) {
                // Tela parada: refinamento em alta qualidade, em lotes.
                tiles = enc.refine(90, 48);
                if tiles.is_empty() {
                    refined = true;
                }
            }
            let cursor = screen.cursor(monitor);
            if !tiles.is_empty() || was_full {
                seq = seq.wrapping_add(1);
                ch.send(&Msg::FrameStart { seq, width: f.width, height: f.height, full: was_full })?;
                let mut batch = Vec::new();
                let mut size = 0;
                for t in tiles {
                    size += t.data.len();
                    batch.push(t);
                    if size > CHUNK {
                        ch.send(&Msg::Tiles(std::mem::take(&mut batch)))?;
                        size = 0;
                    }
                }
                if !batch.is_empty() {
                    ch.send(&Msg::Tiles(batch))?;
                }
                ch.send(&Msg::FrameEnd { seq, cursor })?;
                frames_win.1 += 1;
            } else {
                // Só o cursor.
                ch.send(&Msg::FrameEnd { seq, cursor })?;
            }
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
        if frames_win.0.elapsed() >= Duration::from_secs(1) {
            h.fps.store(frames_win.1, Ordering::Relaxed);
            frames_win = (Instant::now(), 0);
        }
        h.bytes_sent.store(ch.sent, Ordering::Relaxed);
        h.bytes_recv.store(ch.recv, Ordering::Relaxed);
        if last_rx.elapsed() > Duration::from_secs(45) {
            return Err("o visualizador parou de responder".into());
        }
    }
}

// ── visualizador ────────────────────────────────────────────────────

/// Imagem remota compartilhada com a interface.
#[derive(Default)]
pub struct Remote {
    pub dec: Decoder,
    pub version: u64,
    pub cursor: Option<(i32, i32)>,
}

pub enum ViewerEvent {
    Connected { name: String, os: String, sas: String, monitors: Vec<Monitor>, permissions: Permissions },
    Waiting,
    Chat(String),
    File(Transfer),
    Permissions(Permissions),
    /// Conteúdo de uma pasta do anfitrião.
    Listing(Listing),
    /// Aviso do outro lado (ex.: não deu para baixar).
    Notice(String),
    Ended(String),
}

/// Pasta do anfitrião, para o gerenciador de arquivos.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<DirEntry>,
    pub error: Option<String>,
}

pub fn viewer(cfg: ServerCfg, session: u64, secret: [u8; 16], password: String, my_id: String, my_name: String, downloads: PathBuf, remote: Arc<Mutex<Remote>>, events: Sender<ViewerEvent>) -> Handle {
    let (h, rx) = new_handle();
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("elo-visualizador".into())
        .spawn(move || {
            let r = viewer_run(&cfg, session, secret, &password, &my_id, &my_name, &downloads, &remote, &events, &h2, rx);
            let _ = events.send(ViewerEvent::Ended(r.err().unwrap_or_else(|| "sessão encerrada".into())));
        })
        .expect("thread");
    h
}

fn viewer_run(cfg: &ServerCfg, session: u64, secret: [u8; 16], password: &str, my_id: &str, my_name: &str, downloads: &Path, remote: &Arc<Mutex<Remote>>, ev: &Sender<ViewerEvent>, h: &Handle, rx: Receiver<Cmd>) -> Result<(), String> {
    let mut c = join(cfg, session, secret, false)?;
    c.set_read_timeout(Some(Duration::from_secs(30)));
    let keys = viewer_handshake(&mut c, password).map_err(|e| match e {
        elo_proto::Error::Auth => "senha incorreta (ou o computador bloqueou novas tentativas por alguns minutos)".to_string(),
        e => e.to_string(),
    })?;
    let mut ch = Chan { c, fr: Framer::new(), seal: Sealer::new(&keys.send), open: Opener::new(&keys.recv), sent: 0, recv: 0 };
    if keys.mode == AuthMode::Request {
        let _ = ev.send(ViewerEvent::Waiting);
    }
    // Espera o aceite (até 90 s).
    ch.c.set_read_timeout(Some(Duration::from_millis(200)));
    let start = Instant::now();
    let welcome = loop {
        if h.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if let Some(m) = ch.poll()? {
            break m;
        }
        if start.elapsed() > Duration::from_secs(95) {
            return Err("o outro lado não respondeu ao pedido".into());
        }
    };
    let mut perms = match welcome {
        Msg::Welcome { name, os, monitors, permissions } => {
            let _ = ev.send(ViewerEvent::Connected { name, os, sas: keys.sas.clone(), monitors, permissions });
            permissions
        }
        Msg::Bye(r) => return Err(r),
        _ => return Err("resposta inesperada".into()),
    };
    ch.send(&Msg::Hello { name: my_name.into(), from_id: my_id.into() })?;
    ch.c.set_read_timeout(Some(Duration::from_millis(4)));
    let mut clip = Clip::new();
    let mut clip_sync = true;
    let mut last_clip = Instant::now();
    let mut files = Files::default();
    let mut frames_win = (Instant::now(), 0u32);
    let mut last_rx = Instant::now();
    let mut last_ping = Instant::now();
    loop {
        if h.stop.load(Ordering::Relaxed) {
            let _ = ch.send(&Msg::Bye("o visualizador encerrou a sessão".into()));
            return Ok(());
        }
        let mut got = false;
        for _ in 0..256 {
            let Some(m) = ch.poll()? else { break };
            got = true;
            last_rx = Instant::now();
            match m {
                Msg::FrameStart { width, height, .. } => {
                    let mut r = remote.lock().unwrap_or_else(|e| e.into_inner());
                    r.dec.resize(width, height);
                }
                Msg::Tiles(ts) => {
                    let mut r = remote.lock().unwrap_or_else(|e| e.into_inner());
                    for t in &ts {
                        r.dec.apply(t);
                    }
                    r.version += 1;
                }
                Msg::FrameEnd { seq, cursor } => {
                    {
                        let mut r = remote.lock().unwrap_or_else(|e| e.into_inner());
                        if r.cursor != cursor {
                            r.cursor = cursor;
                            r.version += 1;
                        }
                    }
                    ch.send(&Msg::Ack { seq })?;
                    frames_win.1 += 1;
                }
                Msg::Clipboard(t) => {
                    if clip_sync && perms.clipboard {
                        clip.set(&t);
                    }
                }
                Msg::Chat(t) => {
                    let _ = ev.send(ViewerEvent::Chat(t));
                }
                Msg::Permissions(p) => {
                    perms = p;
                    let _ = ev.send(ViewerEvent::Permissions(p));
                }
                Msg::FileOffer { id, name, size, .. } => match files.start_incoming(downloads, id, &name, size) {
                    Ok(_) => ch.send(&Msg::FileAccept { id })?,
                    Err(_) => ch.send(&Msg::FileCancel { id })?,
                },
                Msg::FileAccept { id } => files.accepted(id)?,
                Msg::FileCancel { id } => {
                    let _ = ev.send(ViewerEvent::File(Transfer { id, name: "envio recusado pelo outro lado".into(), done: 0, total: 0, incoming: false, finished: true, path: None }));
                }
                Msg::FileChunk { id, data } => {
                    if let Some(t) = files.chunk(id, &data) {
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                }
                Msg::FileEnd { id } => {
                    if let Some(t) = files.end(id) {
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                }
                Msg::DirListing { path, parent, entries, error } => {
                    let _ = ev.send(ViewerEvent::Listing(Listing { path, parent, entries, error }));
                }
                Msg::Notice(t) => {
                    let _ = ev.send(ViewerEvent::Notice(t));
                }
                Msg::Pong(_) => {}
                Msg::Ping(n) => ch.send(&Msg::Pong(n))?,
                Msg::Bye(r) => return Err(r),
                _ => {}
            }
        }
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Input(i) => {
                    if perms.control {
                        ch.send(&Msg::Input(i))?;
                    }
                }
                Cmd::SecureAttention => ch.send(&Msg::Input(Input::SecureAttention))?,
                Cmd::Chat(t) => ch.send(&Msg::Chat(t))?,
                Cmd::ListDir(path) => ch.send(&Msg::ListDir { path })?,
                Cmd::Restart => ch.send(&Msg::Restart)?,
                Cmd::Download(path) => ch.send(&Msg::FileRequest { path })?,
                Cmd::Upload(p, dir) => match files.offer(&mut ch, &p, Some(dir)) {
                    Ok(t) => {
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                    Err(e) => {
                        let _ = ev.send(ViewerEvent::Notice(format!("não foi possível enviar: {e}")));
                    }
                },
                Cmd::SendFile(p) => match files.offer(&mut ch, &p, None) {
                    Ok(t) => {
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                    Err(e) => {
                        let _ = ev.send(ViewerEvent::Chat(format!("[não foi possível enviar: {e}]")));
                    }
                },
                Cmd::Quality { jpeg, fps } => ch.send(&Msg::Quality { jpeg, fps })?,
                Cmd::Monitor(i) => ch.send(&Msg::SelectMonitor(i))?,
                Cmd::Refresh => ch.send(&Msg::Refresh)?,
                Cmd::Clipboard(on) => clip_sync = on,
                Cmd::Permissions(_) => {}
            }
        }
        let mut progress = Vec::new();
        files.pump(&mut ch, &mut progress)?;
        for t in progress {
            let _ = ev.send(ViewerEvent::File(t));
        }
        if clip_sync && perms.clipboard && last_clip.elapsed() > Duration::from_millis(600) {
            last_clip = Instant::now();
            if let Some(t) = clip.changed() {
                ch.send(&Msg::Clipboard(t))?;
            }
        }
        if last_ping.elapsed() > Duration::from_secs(10) {
            last_ping = Instant::now();
            ch.send(&Msg::Ping(0))?;
        }
        if frames_win.0.elapsed() >= Duration::from_secs(1) {
            h.fps.store(frames_win.1, Ordering::Relaxed);
            frames_win = (Instant::now(), 0);
        }
        h.bytes_sent.store(ch.sent, Ordering::Relaxed);
        h.bytes_recv.store(ch.recv, Ordering::Relaxed);
        if last_rx.elapsed() > Duration::from_secs(45) {
            return Err("o computador remoto parou de responder".into());
        }
        if !got {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
