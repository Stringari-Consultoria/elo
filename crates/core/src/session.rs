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
use elo_proto::msg::{Button, DirEntry, Input, Key, Monitor, Msg, Permissions};
use elo_proto::secure::{AuthMode, Credential, Opener, Peers, Sealer, host_handshake, viewer_handshake};

use crate::net::{Conn, Framer, ServerCfg, connect};
use crate::platform::{self, Clip};

const CHUNK: usize = 192 * 1024;
/// Maior bloco antes do aceite (o aperto de mão e o aviso de boas-vindas são pequenos).
const PRE_ACCEPT_FRAME: usize = 1024 * 1024;
/// Arquivos chegando ao mesmo tempo.
const MAX_INCOMING: usize = 8;
/// Maior texto sincronizado pela área de transferência (o bloco tem 32 MiB:
/// passar disso encerrava a sessão).
pub const MAX_CLIP: usize = 8 * 1024 * 1024;

/// Canal cifrado com leitura não bloqueante.
struct Chan {
    c: Conn,
    fr: Framer,
    seal: Sealer,
    open: Opener,
    pub sent: u64,
    pub recv: u64,
    /// Recursos que o outro lado anunciou (ver `msg::FEATURES`).
    features: Vec<String>,
}

impl Chan {
    fn new(c: Conn, keys: &elo_proto::secure::Keys) -> Self {
        Chan { c, fr: Framer::new(), seal: Sealer::new(&keys.send), open: Opener::new(&keys.recv), sent: 0, recv: 0, features: Vec::new() }
    }

    fn send(&mut self, m: &Msg) -> Result<(), String> {
        self.send_bytes(&postcard::to_stdvec(m).map_err(|e| e.to_string())?)
    }

    /// Mensagem seguida da lista de recursos deste anfitrião.
    fn send_with_features(&mut self, m: &Msg) -> Result<(), String> {
        let mut feats: Vec<&str> = elo_proto::msg::FEATURES.to_vec();
        if platform::fine_scroll() {
            feats.push("rolagem-fina");
        }
        self.send_bytes(&elo_proto::msg::with_features(m, &feats).map_err(|e| e.to_string())?)
    }

    fn send_bytes(&mut self, plain: &[u8]) -> Result<(), String> {
        let data = self.seal.seal(plain).map_err(|e| e.to_string())?;
        self.sent += data.len() as u64 + 4;
        write_frame(&mut self.c, &data).map_err(|e| e.to_string())
    }

    fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }

    fn poll(&mut self) -> Result<Option<Msg>, String> {
        match self.fr.poll(&mut self.c) {
            Ok(Some(f)) => {
                self.recv += f.len() as u64 + 4;
                let plain = self.open.open(&f).map_err(|e| e.to_string())?;
                match elo_proto::msg::features_after(&plain) {
                    Ok((m, feats)) => {
                        if !feats.is_empty() {
                            self.features = feats;
                        }
                        Ok(Some(m))
                    }
                    // Mensagem de uma versão mais nova (o quadro é autenticado, então
                    // não é ruído): ignora em vez de derrubar a sessão.
                    Err(_) => Ok(None),
                }
            }
            Ok(None) => Ok(None),
            Err(e) => Err(if e.kind() == std::io::ErrorKind::UnexpectedEof { "o outro lado encerrou".into() } else { e.to_string() }),
        }
    }
}

/// Entra no relay e espera o par.
fn join(cfg: &ServerCfg, session: u64, secret: [u8; 16], host: bool) -> Result<Conn, String> {
    let mut c = connect(cfg)?;
    // Sem prazo, um outro lado que para de ler prende esta thread no `write`
    // e "Encerrar" não fecha a sessão.
    c.set_write_timeout(Some(Duration::from_secs(30)));
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
    incoming: Vec<Incoming>,
    next: u32,
    offered: Vec<(u32, PathBuf, u64)>,
}

/// Arquivo chegando: fica em `<nome>.part` e só ganha o nome final inteiro.
struct Incoming {
    id: u32,
    file: File,
    part: PathBuf,
    path: PathBuf,
    done: u64,
    total: u64,
}

impl Incoming {
    fn name(&self) -> String {
        self.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Desiste: apaga o parcial.
    fn fail(self, why: &str) -> Transfer {
        let name = self.name();
        let Incoming { id, file, part, done, total, .. } = self;
        drop(file);
        let _ = std::fs::remove_file(part);
        Transfer { id, name, done, total, incoming: true, finished: true, path: None, error: Some(why.into()) }
    }
}

impl Drop for Files {
    /// A sessão acabou no meio: não deixa arquivo pela metade.
    fn drop(&mut self) {
        for x in self.incoming.drain(..) {
            x.fail("");
        }
    }
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
    /// Terminou sem dar certo (com `finished`).
    pub error: Option<String>,
}

/// Nome de arquivo vindo do outro lado: sem caminho, sem caracteres que
/// enganam a leitura (inversão de texto) e sem os nomes que o Windows reserva.
fn safe_file_name(name: &str) -> String {
    let s: String = elo_proto::plain_text(name, 200).chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    // O Windows ignora ponto e espaço no fim ("x.exe." vira "x.exe").
    let s = s.trim_end_matches(['.', ' ']).trim_start().to_string();
    if s.trim_matches('.').is_empty() {
        return "arquivo".into();
    }
    let stem = s.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{s}") } else { s }
}

fn part_of(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Caminho livre em `dir` para `name` (não sobrescreve nada).
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let mut p = dir.join(name);
    let mut k = 1;
    while p.exists() || part_of(&p).exists() {
        let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = Path::new(name).extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
        p = dir.join(format!("{stem} ({k}){ext}"));
        k += 1;
    }
    p
}

impl Files {
    fn offer(&mut self, ch: &mut Chan, path: &Path, dir: Option<String>) -> Result<Transfer, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            return Err("pastas ainda não podem ser enviadas; envie os arquivos".into());
        }
        if !meta.is_file() {
            return Err("só arquivos comuns podem ser enviados".into());
        }
        let size = meta.len();
        self.next += 1;
        let id = self.next;
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "arquivo".into());
        ch.send(&Msg::FileOffer { id, name: name.clone(), size, dir })?;
        self.offered.push((id, path.to_path_buf(), size));
        Ok(Transfer { id, name, done: 0, total: size, incoming: false, finished: false, path: Some(path.to_path_buf()), error: None })
    }

    /// O destino aceitou. Se o arquivo não abrir, só esta transferência falha
    /// (`FileEnd` antes da hora: o destino vê que ficou incompleto).
    fn accepted(&mut self, ch: &mut Chan, id: u32) -> Result<Option<Transfer>, String> {
        let Some(k) = self.offered.iter().position(|x| x.0 == id) else { return Ok(None) };
        let (id, path, size) = self.offered.remove(k);
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match File::open(&path) {
            Ok(f) => {
                self.outgoing.push((id, f, 0, size, name));
                Ok(None)
            }
            Err(e) => {
                ch.send(&Msg::FileEnd { id })?;
                Ok(Some(Transfer { id, name, done: 0, total: size, incoming: false, finished: true, path: None, error: Some(e.to_string()) }))
            }
        }
    }

    /// Envia um pedaço de cada envio em andamento. Devolve progresso.
    fn pump(&mut self, ch: &mut Chan, out: &mut Vec<Transfer>) -> Result<(), String> {
        let mut done = Vec::new();
        for (id, f, sent, total, name) in self.outgoing.iter_mut() {
            // Nunca mais do que o anunciado, mesmo que o arquivo tenha crescido.
            let mut buf = vec![0u8; CHUNK.min(total.saturating_sub(*sent).min(CHUNK as u64) as usize)];
            let (n, error) = match f.read(&mut buf) {
                Ok(0) => (0, (*sent < *total).then(|| "o arquivo diminuiu durante o envio".to_string())),
                Ok(n) => (n, None),
                Err(e) => (0, Some(e.to_string())),
            };
            if n == 0 {
                ch.send(&Msg::FileEnd { id: *id })?;
                done.push(*id);
                out.push(Transfer { id: *id, name: name.clone(), done: *sent, total: *total, incoming: false, finished: true, path: None, error });
                continue;
            }
            buf.truncate(n);
            ch.send(&Msg::FileChunk { id: *id, data: buf })?;
            *sent += n as u64;
            out.push(Transfer { id: *id, name: name.clone(), done: *sent, total: *total, incoming: false, finished: false, path: None, error: None });
        }
        self.outgoing.retain(|x| !done.contains(&x.0));
        Ok(())
    }

    /// O destino recusou ou não conseguiu gravar: para de enviar. O nome vazio
    /// mantém o que já estava na lista.
    fn cancel_send(&mut self, id: u32) -> Transfer {
        self.offered.retain(|x| x.0 != id);
        self.outgoing.retain(|x| x.0 != id);
        Transfer { id, name: String::new(), done: 0, total: 0, incoming: false, finished: true, path: None, error: Some("o outro lado recusou ou não conseguiu gravar o arquivo".into()) }
    }

    /// Permissão de arquivos retirada: para tudo o que estava em andamento.
    fn revoke(&mut self, ch: &mut Chan) -> Result<Vec<Transfer>, String> {
        const WHY: &str = "a permissão de arquivos foi retirada";
        let mut out = Vec::new();
        for (id, ..) in std::mem::take(&mut self.offered) {
            ch.send(&Msg::FileEnd { id })?;
        }
        for (id, _, sent, total, name) in std::mem::take(&mut self.outgoing) {
            ch.send(&Msg::FileEnd { id })?;
            out.push(Transfer { id, name, done: sent, total, incoming: false, finished: true, path: None, error: Some(WHY.into()) });
        }
        for x in std::mem::take(&mut self.incoming) {
            ch.send(&Msg::FileCancel { id: x.id })?;
            out.push(x.fail(WHY));
        }
        Ok(out)
    }

    fn start_incoming(&mut self, dir: &Path, id: u32, name: &str, size: u64) -> Result<PathBuf, String> {
        if self.incoming.len() >= MAX_INCOMING {
            return Err("arquivos demais chegando ao mesmo tempo".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = free_path(dir, &safe_file_name(name));
        let part = part_of(&path);
        let file = File::options().write(true).create_new(true).open(&part).map_err(|e| e.to_string())?;
        self.incoming.push(Incoming { id, file, part, path: path.clone(), done: 0, total: size });
        Ok(path)
    }

    /// Grava um pedaço. Falhou (`error` preenchido): o parcial já foi apagado.
    fn chunk(&mut self, id: u32, data: &[u8]) -> Option<Transfer> {
        let k = self.incoming.iter().position(|x| x.id == id)?;
        let x = &mut self.incoming[k];
        let too_much = x.done.saturating_add(data.len() as u64) > x.total;
        if too_much || x.file.write_all(data).is_err() {
            let why = if too_much { "o outro lado enviou mais dados do que o combinado" } else { "não foi possível gravar o arquivo (disco cheio?)" };
            return Some(self.incoming.remove(k).fail(why));
        }
        x.done += data.len() as u64;
        Some(Transfer { id, name: x.name(), done: x.done, total: x.total, incoming: true, finished: false, path: Some(x.path.clone()), error: None })
    }

    /// Só dá o arquivo como recebido se chegou inteiro e foi para o disco.
    fn end(&mut self, id: u32) -> Option<Transfer> {
        let k = self.incoming.iter().position(|x| x.id == id)?;
        let x = self.incoming.remove(k);
        if x.done != x.total {
            return Some(x.fail("o arquivo chegou incompleto"));
        }
        if x.file.sync_all().is_err() {
            return Some(x.fail("não foi possível gravar o arquivo (disco cheio?)"));
        }
        let name = x.name();
        // Pode ter aparecido um arquivo com esse nome enquanto chegava.
        let path = if x.path.exists() { x.path.parent().map(|d| free_path(d, &name)).unwrap_or_else(|| x.path.clone()) } else { x.path.clone() };
        if std::fs::rename(&x.part, &path).is_err() {
            return Some(x.fail("não foi possível gravar o arquivo (disco cheio?)"));
        }
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name);
        Some(Transfer { id, name, done: x.done, total: x.total, incoming: true, finished: true, path: Some(path), error: None })
    }
}

/// Modificadores que transformam uma letra em atalho.
const SHORTCUT_MODS: [Key; 3] = [Key::Control, Key::Alt, Key::Meta];

/// Texto maior do que isto de uma vez é ignorado: a digitação não dá para
/// interromper. Colar não passa por aqui (vai pela área de transferência).
const MAX_TYPED: usize = 4096;

/// Aplica a entrada do visualizador neste computador e lembra o que ficou
/// apertado, para soltar tudo quando a sessão acaba ou o controle é retirado.
struct Injector {
    ctl: Box<dyn platform::Control>,
    keys: Vec<Key>,
    buttons: Vec<Button>,
}

impl Injector {
    fn new(ctl: Box<dyn platform::Control>) -> Self {
        Self { ctl, keys: Vec::new(), buttons: Vec::new() }
    }

    fn apply(&mut self, input: &Input, monitor: u8) {
        match input {
            Input::Text(t) => {
                if t.chars().count() > MAX_TYPED {
                    return;
                }
                // Texto nunca é digitado com Ctrl, Alt ou Meta apertado: cada letra
                // viraria um atalho no programa aberto (visualizadores antigos colavam assim).
                let mods: Vec<Key> = self.keys.iter().copied().filter(|k| SHORTCUT_MODS.contains(k)).collect();
                for key in &mods {
                    self.ctl.apply(&Input::Key { key: *key, down: false }, monitor);
                }
                self.ctl.apply(input, monitor);
                for key in &mods {
                    self.ctl.apply(&Input::Key { key: *key, down: true }, monitor);
                }
                return;
            }
            Input::Key { key, down } => {
                self.keys.retain(|k| k != key);
                if *down {
                    self.keys.push(*key);
                }
            }
            Input::MouseButton { button, down } => {
                self.buttons.retain(|b| b != button);
                if *down {
                    self.buttons.push(*button);
                }
            }
            _ => {}
        }
        self.ctl.apply(input, monitor);
    }

    /// Solta tudo e descarta o que ainda não foi aplicado.
    fn release_all(&mut self) {
        self.ctl.discard_pending();
        for key in std::mem::take(&mut self.keys).into_iter().rev() {
            self.ctl.apply(&Input::Key { key, down: false }, 0);
        }
        for button in std::mem::take(&mut self.buttons) {
            self.ctl.apply(&Input::MouseButton { button, down: false }, 0);
        }
        self.ctl.settle();
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
    }
}

/// Pasta pessoal do usuário.
fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// Unidades e discos deste computador (caminho de cada um), a raiz primeiro.
fn drives() -> Vec<String> {
    #[cfg(windows)]
    {
        // A: e B: são de disquete (consultar pode travar alguns segundos).
        ('C'..='Z').map(|c| format!("{c}:\\")).filter(|d| Path::new(d).exists()).collect()
    }
    #[cfg(target_os = "macos")]
    {
        let mut v = vec!["/".to_string()];
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            let mut more: Vec<String> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                // "Macintosh HD" é um atalho para a raiz.
                .filter(|p| std::fs::canonicalize(p).map(|c| c != Path::new("/")).unwrap_or(false))
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            more.sort_by_key(|x| x.to_lowercase());
            v.extend(more);
        }
        v
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        mounts(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default())
    }
}

/// Pontos de montagem que são discos de verdade (partições, pendrives, rede),
/// a partir do conteúdo de `/proc/self/mounts`.
#[cfg(not(any(windows, target_os = "macos")))]
fn mounts(table: &str) -> Vec<String> {
    // Espaço e outros caracteres vêm em octal ("\040").
    fn unescape(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
                out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    const NET: &[&str] = &["nfs", "nfs4", "cifs", "smb3", "smbfs", "fuse.sshfs", "fuse.rclone", "9p", "drvfs", "virtiofs"];
    const SKIP: &[&str] = &["/proc", "/sys", "/dev", "/run", "/boot", "/efi", "/snap", "/var", "/tmp", "/usr", "/opt", "/srv", "/etc"];
    let mut v: Vec<String> = Vec::new();
    for line in table.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let [src, at, fs, ..] = f[..] else { continue };
        let at = unescape(at);
        let disk = (src.starts_with("/dev/") && !src.starts_with("/dev/loop") && fs != "squashfs") || fs == "fuseblk" || NET.contains(&fs);
        // /run/media é onde o Fedora e outros montam os pendrives.
        let hidden = SKIP.iter().any(|s| at == *s || at.starts_with(&format!("{s}/"))) && !at.starts_with("/run/media/");
        if (at == "/" || (disk && !hidden)) && !v.contains(&at) {
            v.push(at);
        }
    }
    v.sort_by(|a, b| (a != "/").cmp(&(b != "/")).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
    v
}

/// Lista uma pasta do anfitrião para o gerenciador de arquivos.
fn list_dir(path: &str) -> Msg {
    const MAX: usize = 10_000;
    if path.trim() == elo_proto::msg::DRIVES {
        let entries = drives().into_iter().map(|name| DirEntry { name, dir: true, size: 0, modified: 0 }).collect();
        return Msg::DirListing { path: elo_proto::msg::DRIVES.into(), parent: None, entries, error: None };
    }
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
    /// Mandar o som deste computador a quem pedir (dá para mudar na sessão).
    pub audio: bool,
    /// ID deste computador (entra no aperto de mão: o relay não desvia).
    pub id: String,
    /// Elo Cliente: chave pública dos técnicos da empresa. Com ela, só
    /// técnico aprovado consegue pedir acesso.
    pub tech_ca: Option<[u8; 32]>,
}

/// Motivo de fim para quem pediu acesso sem ser técnico aprovado.
pub const UNVERIFIED: &str = "este computador só aceita técnicos aprovados pela empresa: peça a um administrador para aprovar o seu Elo (Clientes → Técnicos)";

/// Pedidos sem senha abandonados no meio do aperto de mão. Quem está no meio
/// da conexão abandona para tentar de novo o código de verificação; passando
/// do limite, pedidos sem senha param por alguns minutos.
static ABANDONED: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
const MAX_ABANDONED: usize = 30;

fn requests_allowed() -> bool {
    let mut a = ABANDONED.lock().unwrap_or_else(|e| e.into_inner());
    a.retain(|t| t.elapsed() < Duration::from_secs(600));
    a.len() < MAX_ABANDONED
}

/// Ligar o som sozinho ao conectar (os testes desligam: anfitrião e
/// visualizador no mesmo computador fariam eco).
pub static AUDIO_ON_START: AtomicBool = AtomicBool::new(true);

/// Motivo de fim quando quem pediu acesso desistiu antes da resposta.
pub const GAVE_UP: &str = "quem pediu desistiu do pedido";

pub enum HostEvent {
    /// Pedido sem senha: responda com as permissões (ou `None` para recusar).
    /// `verified`: nome do técnico aprovado pela empresa (certificado conferido).
    Ask { from_id: String, from_name: String, sas: String, reply: Sender<Option<Permissions>>, verified: Option<String> },
    /// `password`: qual senha da lista de `HostOptions` foi usada (`None` = aceite na tela).
    Started { from_id: String, from_name: String, sas: String, password: Option<u8>, verified: Option<String> },
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
    /// Vai colar lá: o texto que está na área de transferência daqui agora.
    PasteText(String),
    SecureAttention,
    /// Visualizador: ouvir ou não o som de lá. Anfitrião: permitir ou não.
    Audio(bool),
    /// Volume do som recebido (0 a 1,5).
    Volume(f32),
}

fn new_handle() -> (Handle, Receiver<Cmd>) {
    let (tx, rx) = channel();
    (Handle { stop: Arc::new(AtomicBool::new(false)), cmd: tx, bytes_sent: Arc::new(AtomicU64::new(0)), bytes_recv: Arc::new(AtomicU64::new(0)), fps: Arc::new(AtomicU32::new(0)) }, rx)
}

/// Tentativas erradas recentes e de quem (bloqueio contra força bruta).
static FAILS: Mutex<Vec<(Instant, String)>> = Mutex::new(Vec::new());
/// Erros seguidos de um mesmo ID que bloqueiam as senhas para ele.
const FAILS_PER_ORIGIN: usize = 5;
/// Erros de todos juntos que bloqueiam as senhas para todos (ataque de muitos IDs).
const FAILS_TOTAL: usize = 20;

/// Senhas bloqueadas para este ID (5 erros dele ou 20 de todos em 5 minutos).
/// Pedir acesso com aceite na tela continua valendo: o bloqueio não tranca
/// o computador para quem está do outro lado esperando ajuda.
pub fn host_locked(from_id: &str) -> bool {
    let mut f = FAILS.lock().unwrap_or_else(|e| e.into_inner());
    f.retain(|(t, _)| t.elapsed() < Duration::from_secs(300));
    f.len() >= FAILS_TOTAL || f.iter().filter(|(_, id)| id == from_id).count() >= FAILS_PER_ORIGIN
}

fn note_fail(from_id: &str) {
    FAILS.lock().unwrap_or_else(|e| e.into_inner()).push((Instant::now(), from_id.to_string()));
}

pub fn host(cfg: ServerCfg, session: u64, secret: [u8; 16], from_id: String, from_name: String, opt: HostOptions, events: Sender<HostEvent>) -> Handle {
    let (h, rx) = new_handle();
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("elo-anfitriao".into())
        .spawn(move || {
            // O nome vem de quem pede: numa linha só, sem truques de texto.
            let from_name = elo_proto::plain_text(&from_name, 80);
            let r = host_run(&cfg, session, secret, &from_id, &from_name, &opt, &events, &h2, rx);
            let _ = events.send(HostEvent::Ended(r.err().unwrap_or_else(|| "sessão encerrada".into())));
        })
        .expect("thread");
    h
}

fn host_run(cfg: &ServerCfg, session: u64, secret: [u8; 16], from_id: &str, from_name: &str, opt: &HostOptions, ev: &Sender<HostEvent>, h: &Handle, rx: Receiver<Cmd>) -> Result<(), String> {
    let mut c = join(cfg, session, secret, true)?;
    c.set_read_timeout(Some(Duration::from_secs(30)));
    let locked = host_locked(from_id);
    let peers = Peers { session, viewer: from_id, host: &opt.id };
    let keys = match host_handshake(&mut c, if locked { &[] } else { &opt.passwords }, peers, requests_allowed()) {
        Ok(k) => k,
        Err(elo_proto::Error::Abandoned) => {
            ABANDONED.lock().unwrap_or_else(|e| e.into_inner()).push(Instant::now());
            return Err(GAVE_UP.into());
        }
        Err(e) => {
            if locked {
                return Err("senha bloqueada por alguns minutos: muitas tentativas erradas".into());
            }
            if matches!(e, elo_proto::Error::Auth) {
                note_fail(from_id);
                let _ = ev.send(HostEvent::AuthFailed { from_id: from_id.into() });
                return Err("senha incorreta".into());
            }
            return Err(e.to_string());
        }
    };
    let mut ch = Chan::new(c, &keys);
    let mut perms = opt.default_permissions;
    // Antes do aceite, quem pediu só sabe o ID: nada de blocos grandes.
    ch.fr.set_max(PRE_ACCEPT_FRAME);
    // Técnico aprovado: o certificado bate com a chave da empresa e assina
    // esta sessão (ninguém no meio consegue repassar a assinatura).
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let verified = match (&opt.tech_ca, &keys.proof) {
        (Some(ca), Some((cert, sig))) => elo_proto::cert::check(ca, now, cert, sig, &keys.binding).map(|c| c.name),
        _ => None,
    };
    if keys.mode == AuthMode::Request && opt.tech_ca.is_some() && verified.is_none() {
        let _ = ch.send(&Msg::Bye(UNVERIFIED.into()));
        return Err("pedido de acesso recusado: quem pediu não é um técnico aprovado da empresa".into());
    }
    let from_name = match &verified {
        Some(n) => format!("{} (técnico)", elo_proto::plain_text(n, 64)),
        None => from_name.to_string(),
    };
    let from_name = from_name.as_str();
    if keys.mode == AuthMode::Request {
        let (tx, rrx) = channel();
        let _ = ev.send(HostEvent::Ask { from_id: from_id.into(), from_name: from_name.into(), sas: keys.sas.clone(), reply: tx, verified: verified.clone() });
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
    ch.fr.set_max(elo_proto::frame::MAX_FRAME);
    let mut screen = platform::open_screen()?;
    let mut control = platform::open_control().ok().map(Injector::new);
    let mut clip = Clip::new();
    let monitors = screen.monitors();
    ch.send_with_features(&Msg::Welcome { name: opt.name.clone(), os: platform::os_name(), monitors, permissions: perms })?;
    let _ = ev.send(HostEvent::Started { from_id: from_id.into(), from_name: from_name.into(), sas: keys.sas.clone(), password: (keys.mode == AuthMode::Password).then_some(keys.which), verified: verified.clone() });
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
    // Som: manda só se quem acessa pediu e se aqui está permitido.
    let mut audio_allowed = opt.audio;
    let mut audio_wanted = false;
    let mut audio_failed = false;
    let mut capture: Option<(crate::audio::Capture, crate::audio::Encoder)> = None;
    let mut pcm = Vec::new();
    loop {
        if h.stop.load(Ordering::Relaxed) {
            let _ = ch.send(&Msg::Bye("o anfitrião encerrou a sessão".into()));
            return Ok(());
        }
        // Mensagens do visualizador.
        for _ in 0..64 {
            // "Encerrar" vale também no meio de um lote de entradas.
            if h.stop.load(Ordering::Relaxed) {
                break;
            }
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
                Msg::FileAccept { id } if perms.files => {
                    if let Some(t) = files.accepted(&mut ch, id)? {
                        let _ = ev.send(HostEvent::File(t));
                    }
                }
                Msg::FileCancel { id } => {
                    let _ = ev.send(HostEvent::File(files.cancel_send(id)));
                }
                Msg::FileChunk { id, data } if perms.files => {
                    if let Some(t) = files.chunk(id, &data) {
                        if t.error.is_some() {
                            ch.send(&Msg::FileCancel { id })?;
                        }
                        let _ = ev.send(HostEvent::File(t));
                    }
                }
                Msg::FileEnd { id } if perms.files => {
                    if let Some(t) = files.end(id) {
                        if t.error.is_some() {
                            ch.send(&Msg::FileCancel { id })?;
                        }
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
                Msg::AudioStart => {
                    audio_wanted = true;
                    audio_failed = false;
                    if !audio_allowed {
                        ch.send(&Msg::Notice("o som está desligado do outro lado".into()))?;
                    }
                }
                Msg::AudioStop => audio_wanted = false,
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
                    // Retirou o controle no meio de um arrasto ou atalho: não fica nada apertado.
                    if !p.control
                        && let Some(c) = control.as_mut()
                    {
                        c.release_all();
                    }
                    if !p.files {
                        for t in files.revoke(&mut ch)? {
                            let _ = ev.send(HostEvent::File(t));
                        }
                    }
                    perms = p;
                    ch.send(&Msg::Permissions(p))?;
                }
                Cmd::Audio(on) => {
                    audio_allowed = on;
                    audio_failed = false;
                    if audio_wanted {
                        ch.send(&Msg::Notice(if on { "o som foi liberado do outro lado".into() } else { "o som foi desligado do outro lado".into() }))?;
                    }
                }
                _ => {}
            }
        }
        // Som: captura, comprime e manda (pacotes de 20 ms).
        if audio_wanted && audio_allowed && !audio_failed {
            if capture.is_none() {
                match crate::audio::Capture::open().and_then(|c| Ok((c, crate::audio::Encoder::new()?))) {
                    Ok(c) => capture = Some(c),
                    Err(e) => {
                        audio_failed = true;
                        ch.send(&Msg::Notice(format!("sem som do outro computador: {e}")))?;
                    }
                }
            }
            if let Some((cap, enc)) = capture.as_mut() {
                pcm.clear();
                cap.read(&mut pcm);
                for p in enc.push(&pcm) {
                    ch.send(&Msg::Audio(p))?;
                }
            }
        } else if capture.is_some() {
            capture = None;
        }
        let mut progress = Vec::new();
        files.pump(&mut ch, &mut progress)?;
        for t in progress {
            let _ = ev.send(HostEvent::File(t));
        }
        // Área de transferência local → visualizador.
        if perms.clipboard && last_clip.elapsed() > Duration::from_millis(600) {
            last_clip = Instant::now();
            // Texto enorme não vai: passaria do tamanho do bloco e derrubaria a sessão.
            if let Some(t) = clip.changed().filter(|t| t.len() <= MAX_CLIP) {
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
    /// `password`: qual senha do outro lado valeu (0 = a temporária, 1 = a fixa; `None` = aceite).
    Connected { name: String, os: String, sas: String, monitors: Vec<Monitor>, permissions: Permissions, password: Option<u8> },
    /// Se o outro lado sabe mandar som (versões antigas não sabem).
    Audio { available: bool },
    /// Recursos que o anfitrião anunciou ("audio", "rolagem-fina"…).
    Features(Vec<String>),
    /// Esperando o aceite. `sas`: o código a conferir com quem vai aceitar.
    Waiting { sas: String },
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

/// `target_id`: o ID digitado (entra no aperto de mão); `credential`: técnico
/// de empresa, para provar quem é ao pedir acesso a um Elo Cliente.
pub fn viewer(cfg: ServerCfg, session: u64, secret: [u8; 16], password: String, my_id: String, my_name: String, downloads: PathBuf, remote: Arc<Mutex<Remote>>, events: Sender<ViewerEvent>, target_id: String, credential: Option<Credential>) -> Handle {
    let (h, rx) = new_handle();
    let h2 = h.clone();
    std::thread::Builder::new()
        .name("elo-visualizador".into())
        .spawn(move || {
            let who = Who { my_id: &my_id, my_name: &my_name, target_id: &target_id, credential: credential.as_ref() };
            let r = viewer_run(&cfg, session, secret, &password, who, &downloads, &remote, &events, &h2, rx);
            let _ = events.send(ViewerEvent::Ended(r.err().unwrap_or_else(|| "sessão encerrada".into())));
        })
        .expect("thread");
    h
}

/// Quem acessa, para quem e com que credencial.
struct Who<'a> {
    my_id: &'a str,
    my_name: &'a str,
    target_id: &'a str,
    credential: Option<&'a Credential>,
}

fn viewer_run(cfg: &ServerCfg, session: u64, secret: [u8; 16], password: &str, who: Who, downloads: &Path, remote: &Arc<Mutex<Remote>>, ev: &Sender<ViewerEvent>, h: &Handle, rx: Receiver<Cmd>) -> Result<(), String> {
    let Who { my_id, my_name, target_id, credential } = who;
    let mut c = join(cfg, session, secret, false)?;
    c.set_read_timeout(Some(Duration::from_secs(30)));
    let peers = Peers { session, viewer: my_id, host: target_id };
    let keys = viewer_handshake(&mut c, password, peers, credential).map_err(|e| match e {
        elo_proto::Error::Auth if password.is_empty() => "o outro computador recusou o pedido (muitas tentativas em pouco tempo, ou a conexão foi desviada): tente de novo em alguns minutos".to_string(),
        elo_proto::Error::Auth => "senha incorreta (ou o computador bloqueou novas tentativas por alguns minutos)".to_string(),
        e => e.to_string(),
    })?;
    let mut ch = Chan::new(c, &keys);
    ch.fr.set_max(PRE_ACCEPT_FRAME);
    if keys.mode == AuthMode::Request {
        let _ = ev.send(ViewerEvent::Waiting { sas: keys.sas.clone() });
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
            let _ = ev.send(ViewerEvent::Connected { name, os, sas: keys.sas.clone(), monitors, permissions, password: (keys.mode == AuthMode::Password).then_some(keys.which) });
            permissions
        }
        Msg::Bye(r) => return Err(r),
        _ => return Err("resposta inesperada".into()),
    };
    ch.fr.set_max(elo_proto::frame::MAX_FRAME);
    ch.send(&Msg::Hello { name: my_name.into(), from_id: my_id.into() })?;
    ch.c.set_read_timeout(Some(Duration::from_millis(4)));
    // Som: liga sozinho se o outro lado sabe mandar.
    let audio_ok = ch.has("audio");
    let _ = ev.send(ViewerEvent::Audio { available: audio_ok });
    let _ = ev.send(ViewerEvent::Features(ch.features.clone()));
    let mut volume = 1.0f32;
    let mut player: Option<crate::audio::Player> = None;
    let start_audio = |ch: &mut Chan, volume: f32| -> Result<Option<crate::audio::Player>, String> {
        match crate::audio::Player::open(volume) {
            Ok(p) => {
                ch.send(&Msg::AudioStart)?;
                Ok(Some(p))
            }
            Err(e) => {
                let _ = ev.send(ViewerEvent::Notice(format!("não dá para tocar o som aqui: {e}")));
                Ok(None)
            }
        }
    };
    if audio_ok && AUDIO_ON_START.load(Ordering::Relaxed) {
        player = start_audio(&mut ch, volume)?;
    }
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
                    if !r.dec.resize(width, height) {
                        return Err(format!("o outro lado mandou uma tela de tamanho inválido ({width} x {height})"));
                    }
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
                Msg::FileAccept { id } => {
                    if let Some(t) = files.accepted(&mut ch, id)? {
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                }
                Msg::FileCancel { id } => {
                    let _ = ev.send(ViewerEvent::File(files.cancel_send(id)));
                }
                Msg::FileChunk { id, data } => {
                    if let Some(t) = files.chunk(id, &data) {
                        if t.error.is_some() {
                            ch.send(&Msg::FileCancel { id })?;
                        }
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                }
                Msg::FileEnd { id } => {
                    if let Some(t) = files.end(id) {
                        if t.error.is_some() {
                            ch.send(&Msg::FileCancel { id })?;
                        }
                        let _ = ev.send(ViewerEvent::File(t));
                    }
                }
                Msg::DirListing { path, parent, entries, error } => {
                    let _ = ev.send(ViewerEvent::Listing(Listing { path, parent, entries, error }));
                }
                Msg::Notice(t) => {
                    let _ = ev.send(ViewerEvent::Notice(t));
                }
                Msg::Audio(packet) => {
                    if let Some(p) = player.as_mut() {
                        p.push(&packet);
                    }
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
                Cmd::PasteText(t) => {
                    // Só manda se a sincronia permite e o outro lado ainda não tem esse texto.
                    if t.len() > MAX_CLIP {
                        let _ = ev.send(ViewerEvent::Notice("o texto copiado é grande demais para colar lá (mais de 8 MB)".into()));
                    } else if clip_sync && perms.clipboard && clip.note_sent(&t) {
                        ch.send(&Msg::Clipboard(t))?;
                    }
                }
                Cmd::Permissions(_) => {}
                Cmd::Audio(on) => {
                    if !audio_ok {
                        let _ = ev.send(ViewerEvent::Notice("o Elo do outro computador ainda não manda som: atualize-o".into()));
                    } else if on && player.is_none() {
                        player = start_audio(&mut ch, volume)?;
                    } else if !on && player.is_some() {
                        player = None;
                        ch.send(&Msg::AudioStop)?;
                    }
                }
                Cmd::Volume(v) => {
                    volume = v;
                    if let Some(p) = &player {
                        p.set_volume(v);
                    }
                }
            }
        }
        let mut progress = Vec::new();
        files.pump(&mut ch, &mut progress)?;
        for t in progress {
            let _ = ev.send(ViewerEvent::File(t));
        }
        if clip_sync && perms.clipboard && last_clip.elapsed() > Duration::from_millis(600) {
            last_clip = Instant::now();
            // Texto enorme não vai: passaria do tamanho do bloco e derrubaria a sessão.
            if let Some(t) = clip.changed().filter(|t| t.len() <= MAX_CLIP) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<Input>>>);

    impl platform::Control for Fake {
        fn apply(&mut self, input: &Input, _monitor: u8) {
            self.0.lock().unwrap().push(input.clone());
        }
    }

    impl Fake {
        fn seen(&self) -> Vec<Input> {
            self.0.lock().unwrap().clone()
        }
    }

    fn key(key: Key, down: bool) -> Input {
        Input::Key { key, down }
    }

    #[test]
    fn text_never_goes_out_with_a_shortcut_modifier_held() {
        let fake = Fake::default();
        let mut inj = Injector::new(Box::new(fake.clone()));
        let paste = Input::Text("a-b".into());
        for i in [key(Key::Control, true), paste.clone(), key(Key::Control, false)] {
            inj.apply(&i, 0);
        }
        assert_eq!(fake.seen(), [key(Key::Control, true), key(Key::Control, false), paste, key(Key::Control, true), key(Key::Control, false)]);
    }

    #[test]
    fn shortcuts_and_shift_stay_as_they_are() {
        let fake = Fake::default();
        let mut inj = Injector::new(Box::new(fake.clone()));
        let typed = [key(Key::Shift, true), Input::Text("A".into()), key(Key::Shift, false), key(Key::Control, true), key(Key::Char('s'), true), key(Key::Char('s'), false), key(Key::Control, false)];
        for i in &typed {
            inj.apply(i, 0);
        }
        assert_eq!(fake.seen(), typed);
    }

    #[test]
    fn huge_text_is_ignored() {
        let fake = Fake::default();
        let mut inj = Injector::new(Box::new(fake.clone()));
        inj.apply(&Input::Text("x".repeat(MAX_TYPED + 1)), 0);
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn everything_is_released_when_the_session_ends() {
        let fake = Fake::default();
        {
            let mut inj = Injector::new(Box::new(fake.clone()));
            inj.apply(&key(Key::Alt, true), 0);
            inj.apply(&Input::MouseButton { button: Button::Left, down: true }, 0);
            inj.apply(&key(Key::Char('f'), true), 0);
            inj.apply(&key(Key::Char('f'), false), 0);
        }
        let seen = fake.seen();
        assert_eq!(seen[seen.len() - 2..], [key(Key::Alt, false), Input::MouseButton { button: Button::Left, down: false }]);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("elo-arquivos-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_only_gets_its_name_when_complete() {
        let d = temp_dir("completo");
        let mut f = Files::default();
        let path = f.start_incoming(&d, 1, "a.txt", 3).unwrap();
        assert!(!path.exists() && part_of(&path).exists());
        assert!(f.chunk(1, b"abc").unwrap().error.is_none());
        let t = f.end(1).unwrap();
        assert!(t.error.is_none() && t.finished);
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert!(!part_of(&path).exists());
    }

    #[test]
    fn incomplete_or_oversized_file_is_removed_and_reported() {
        let d = temp_dir("incompleto");
        let mut f = Files::default();
        let curto = f.start_incoming(&d, 1, "curto.txt", 10).unwrap();
        f.chunk(1, b"abc");
        assert!(f.end(1).unwrap().error.is_some());
        assert!(!curto.exists() && !part_of(&curto).exists());
        let longo = f.start_incoming(&d, 2, "longo.txt", 2).unwrap();
        assert!(f.chunk(2, b"abc").unwrap().error.is_some());
        assert!(!longo.exists() && !part_of(&longo).exists());
    }

    #[test]
    fn session_ending_midway_leaves_no_partial_file() {
        let d = temp_dir("queda");
        let path = {
            let mut f = Files::default();
            let p = f.start_incoming(&d, 1, "b.txt", 10).unwrap();
            f.chunk(1, b"abc");
            p
        };
        assert!(!path.exists() && !part_of(&path).exists());
    }

    #[test]
    fn same_name_never_overwrites() {
        let d = temp_dir("nome");
        std::fs::write(d.join("c.txt"), b"antigo").unwrap();
        let mut f = Files::default();
        let p = f.start_incoming(&d, 1, "c.txt", 1).unwrap();
        f.chunk(1, b"n");
        let t = f.end(1).unwrap();
        assert_ne!(p, d.join("c.txt"));
        assert_eq!(std::fs::read(d.join("c.txt")).unwrap(), b"antigo");
        assert_eq!(std::fs::read(t.path.unwrap()).unwrap(), b"n");
    }

    #[test]
    fn received_names_cannot_fool_or_escape() {
        assert_eq!(super::safe_file_name("fatura\u{202E}fdp.exe"), "faturafdp.exe");
        assert_eq!(super::safe_file_name("../../.bashrc"), ".._.._.bashrc");
        assert_eq!(super::safe_file_name("x.exe. . "), "x.exe");
        assert_eq!(super::safe_file_name("CON.txt"), "_CON.txt");
        assert_eq!(super::safe_file_name("com1"), "_com1");
        assert_eq!(super::safe_file_name("console.txt"), "console.txt");
        assert_eq!(super::safe_file_name(".."), "arquivo");
        assert_eq!(super::safe_file_name("a\nb"), "a b");
        assert!(super::safe_file_name(&"a".repeat(1000)).len() <= 200);
    }

    #[test]
    fn too_many_files_at_once_are_refused() {
        let d = temp_dir("muitos");
        let mut f = Files::default();
        for id in 0..super::MAX_INCOMING as u32 {
            f.start_incoming(&d, id, "x", 10).unwrap();
        }
        assert!(f.start_incoming(&d, 99, "x", 10).is_err());
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn mounts_only_real_disks() {
        let table = "/dev/nvme0n1p3 / ext4 rw 0 0\nproc /proc proc rw 0 0\n/dev/loop0 /snap/bare/5 squashfs ro 0 0\n/dev/nvme0n1p2 /boot/efi vfat rw 0 0\n/dev/sdb1 /media/gabriel/ARQUIVOS ext4 rw 0 0\n/dev/sda1 /media/gabriel/BACKUPS ext4 rw 0 0\nlxcfs /var/lib/lxcfs fuse.lxcfs rw 0 0\ntmpfs /run/user/1000 tmpfs rw 0 0\n/dev/sdc1 /run/media/ana/MEU\\040PENDRIVE vfat rw 0 0\n//nas/fotos /mnt/nas cifs rw 0 0\n/dev/nvme0n1p4 /home ext4 rw 0 0\n/dev/sdb1 /media/gabriel/ARQUIVOS ext4 rw 0 0\n";
        assert_eq!(super::mounts(table), ["/", "/home", "/media/gabriel/ARQUIVOS", "/media/gabriel/BACKUPS", "/mnt/nas", "/run/media/ana/MEU PENDRIVE"]);
    }
}
