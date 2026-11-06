//! Wayland (GNOME, KDE e outros): captura e controle pelo portal do sistema
//! (xdg-desktop-portal: RemoteDesktop + ScreenCast) e os quadros pelo PipeWire.
//!
//! Na primeira vez, o sistema pergunta na tela do computador acessado se
//! pode compartilhar; o Elo guarda a permissão (restore token) e, nas
//! próximas, o portal não pergunta de novo (até alguém revogar).

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions, StartOptions};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use elo_proto::msg::{Button, Input, Key, Monitor};

use crate::platform::{Control, Frame, Screen};

/// Sessão gráfica Wayland? (`ELO_PORTAL=1` força o portal, para testes.)
pub fn is_wayland() -> bool {
    std::env::var("ELO_PORTAL").is_ok_and(|v| v == "1") || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
}

/// Um monitor compartilhado pelo portal.
struct StreamMeta {
    node: u32,
    width: u32,
    height: u32,
}

enum Cmd {
    Motion(u32, f64, f64),
    Button(i32, bool),
    Axis(f64, f64),
    Discrete(Axis, i32),
    Keysym(i32, bool),
}

/// Sessão do portal (compartilhada entre a captura e o controle).
struct Portal {
    streams: Vec<StreamMeta>,
    /// Último quadro de cada monitor.
    frames: Vec<Arc<Mutex<Option<Frame>>>>,
    cmd: tokio::sync::mpsc::UnboundedSender<Cmd>,
    quit_pw: Mutex<Option<pipewire::channel::Sender<()>>>,
    stop: Arc<AtomicBool>,
    /// Alguém está capturando (sem ninguém, os quadros são descartados).
    wanted: Arc<AtomicBool>,
}

impl Drop for Portal {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(q) = self.quit_pw.lock().ok().and_then(|mut q| q.take()) {
            let _ = q.send(());
        }
    }
}

/// A sessão aberta pela captura; o controle usa a mesma.
static CURRENT: Mutex<Weak<Portal>> = Mutex::new(Weak::new());

/// Depois de um acesso, a sessão do portal fica aberta mais um pouco: quem
/// reconectar logo (queda de rede, outra aba) não faz o portal perguntar de
/// novo. `(sessão, até quando)`.
static KEEP: Mutex<Option<(Arc<Portal>, Instant)>> = Mutex::new(None);
const KEEP_FOR: Duration = Duration::from_secs(300);

fn token_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/state")))?;
    Some(base.join("elo").join("portal-token"))
}

/// Abre a sessão do portal: pergunta ao usuário (ou usa a permissão
/// guardada), pega os monitores e o acesso ao PipeWire.
async fn open() -> Result<(RemoteDesktop, Session<RemoteDesktop>, Vec<StreamMeta>, OwnedFd), String> {
    let token = token_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if let Some(t) = token.as_deref() {
        // Com a permissão guardada, o portal responde na hora. Se voltar sem
        // telas (o GNOME 46 restaura o mouse e o teclado, mas não a captura),
        // travar ou falhar, desiste dela e pergunta de novo.
        match tokio::time::timeout(Duration::from_secs(10), open_with(Some(t))).await {
            Ok(Ok(Some(ok))) => return Ok(ok),
            Ok(Ok(None)) => eprintln!("[portal] a permissão guardada voltou sem telas; perguntando de novo"),
            Ok(Err(e)) => eprintln!("[portal] a permissão guardada falhou ({e}); perguntando de novo"),
            Err(_) => eprintln!("[portal] a permissão guardada não respondeu; perguntando de novo"),
        }
        if let Some(f) = token_file() {
            let _ = std::fs::remove_file(f);
        }
    }
    open_with(None).await?.ok_or_else(|| "nenhuma tela foi compartilhada".to_string())
}

/// `Ok(None)`: aceitou, mas sem telas (permissão guardada incompleta).
async fn open_with(token: Option<&str>) -> Result<Option<(RemoteDesktop, Session<RemoteDesktop>, Vec<StreamMeta>, OwnedFd)>, String> {
    let err = |what: &str, e: ashpd::Error| format!("{what}: {e}");
    let rd = RemoteDesktop::new().await.map_err(|e| err("portal de acesso remoto indisponível", e))?;
    let sc = Screencast::new().await.map_err(|e| err("portal de captura de tela indisponível", e))?;
    let session = rd.create_session(Default::default()).await.map_err(|e| err("sessão do portal", e))?;
    eprintln!("[portal] sessão criada");
    rd.select_devices(&session, SelectDevicesOptions::default().set_devices(DeviceType::KEYBOARD | DeviceType::POINTER).set_persist_mode(PersistMode::ExplicitlyRevoked).set_restore_token(token))
        .await
        .map_err(|e| err("escolha dos dispositivos", e))?;
    eprintln!("[portal] dispositivos escolhidos (permissão guardada: {})", token.is_some());
    sc.select_sources(&session, SelectSourcesOptions::default().set_cursor_mode(CursorMode::EMBEDDED).set_sources(SourceType::MONITOR).set_multiple(true))
        .await
        .map_err(|e| err("escolha das telas", e))?;
    eprintln!("[portal] telas escolhidas; esperando a resposta do usuário");
    let started = rd.start(&session, None, StartOptions::default()).await.map_err(|e| err("início do compartilhamento", e))?;
    let sel = started.response().map_err(|e| err("o compartilhamento da tela foi recusado neste computador", e))?;
    eprintln!("[portal] compartilhamento aceito ({} tela(s))", sel.streams().len());
    if let (Some(t), Some(f)) = (sel.restore_token(), token_file()) {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, t);
    }
    let streams: Vec<StreamMeta> = sel
        .streams()
        .iter()
        .map(|s| {
            let (w, h) = s.size().unwrap_or((0, 0));
            StreamMeta { node: s.pipe_wire_node_id(), width: w.max(0) as u32, height: h.max(0) as u32 }
        })
        .collect();
    if streams.is_empty() {
        let _ = session.close().await;
        return if token.is_some() { Ok(None) } else { Err("nenhuma tela foi compartilhada".into()) };
    }
    let fd = sc.open_pipe_wire_remote(&session, Default::default()).await.map_err(|e| err("acesso ao PipeWire", e))?;
    Ok(Some((rd, session, streams, fd)))
}

async fn apply(rd: &RemoteDesktop, s: &Session<RemoteDesktop>, c: Cmd) {
    let r = match c {
        Cmd::Motion(node, x, y) => rd.notify_pointer_motion_absolute(s, node, x, y, Default::default()).await,
        Cmd::Button(b, down) => rd.notify_pointer_button(s, b, if down { KeyState::Pressed } else { KeyState::Released }, Default::default()).await,
        Cmd::Axis(dx, dy) => rd.notify_pointer_axis(s, dx, dy, Default::default()).await,
        Cmd::Discrete(axis, steps) => rd.notify_pointer_axis_discrete(s, axis, steps, Default::default()).await,
        Cmd::Keysym(k, down) => rd.notify_keyboard_keysym(s, k, if down { KeyState::Pressed } else { KeyState::Released }, Default::default()).await,
    };
    // Raro (ex.: o usuário revogou o controle); um aviso por tipo de erro basta.
    if let Err(e) = r {
        static AVISADO: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let m = e.to_string();
        if let Ok(mut v) = AVISADO.lock()
            && !v.contains(&m)
        {
            eprintln!("[portal] entrada recusada: {m}");
            v.push(m);
        }
    }
}

fn start() -> Result<Arc<Portal>, String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(Vec<StreamMeta>, OwnedFd), String>>();
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    std::thread::Builder::new()
        .name("elo-portal".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            rt.block_on(async move {
                match open().await {
                    Ok((rd, session, streams, fd)) => {
                        let _ = ready_tx.send(Ok((streams, fd)));
                        // Entradas até a sessão acabar (o `Portal` é descartado).
                        while let Some(c) = cmd_rx.recv().await {
                            apply(&rd, &session, c).await;
                        }
                        let _ = session.close().await;
                        eprintln!("[portal] sessão fechada");
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        })
        .map_err(|e| e.to_string())?;
    // A primeira vez espera alguém aceitar na tela.
    let (streams, fd) = ready_rx.recv_timeout(Duration::from_secs(120)).map_err(|_| "ninguém respondeu ao pedido de compartilhar a tela neste computador".to_string())??;
    let frames: Vec<Arc<Mutex<Option<Frame>>>> = streams.iter().map(|_| Arc::new(Mutex::new(None))).collect();
    let stop = Arc::new(AtomicBool::new(false));
    let wanted = Arc::new(AtomicBool::new(true));
    let (quit_tx, quit_rx) = pipewire::channel::channel::<()>();
    let nodes: Vec<u32> = streams.iter().map(|s| s.node).collect();
    let slots = frames.clone();
    let w = wanted.clone();
    std::thread::Builder::new()
        .name("elo-pipewire".into())
        .spawn(move || {
            if let Err(e) = pipewire_loop(fd, nodes, slots, w, quit_rx) {
                eprintln!("PipeWire: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(Portal { streams, frames, cmd: cmd_tx, quit_pw: Mutex::new(Some(quit_tx)), stop, wanted }))
}

/// Recebe os quadros de cada monitor e guarda o último, em RGBA.
fn pipewire_loop(fd: OwnedFd, nodes: Vec<u32>, slots: Vec<Arc<Mutex<Option<Frame>>>>, wanted: Arc<AtomicBool>, quit: pipewire::channel::Receiver<()>) -> Result<(), String> {
    use pipewire as pw;
    use pw::spa;
    use spa::param::video::{VideoFormat, VideoInfoRaw};
    use spa::pod::Pod;

    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| e.to_string())?;
    let core = context.connect_fd(fd, None).map_err(|e| e.to_string())?;
    let ml = mainloop.clone();
    let _quit = quit.attach(mainloop.loop_(), move |_| ml.quit());

    // Formatos aceitos: RGB de 32 bits sem compressão (o que o GNOME e o KDE mandam).
    let obj = pw::spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(spa::param::format::FormatProperties::MediaType, Id, spa::param::format::MediaType::Video),
        pw::spa::pod::property!(spa::param::format::FormatProperties::MediaSubtype, Id, spa::param::format::MediaSubtype::Raw),
        pw::spa::pod::property!(spa::param::format::FormatProperties::VideoFormat, Choice, Enum, Id, VideoFormat::BGRx, VideoFormat::BGRx, VideoFormat::BGRA, VideoFormat::RGBx, VideoFormat::RGBA, VideoFormat::xRGB, VideoFormat::ARGB),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle { width: 1920, height: 1080 },
            spa::utils::Rectangle { width: 1, height: 1 },
            spa::utils::Rectangle { width: 8192, height: 8192 }
        ),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 120, denom: 1 }
        ),
    );
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &pw::spa::pod::Value::Object(obj)).map_err(|e| format!("{e:?}"))?.0.into_inner();

    let mut keep = Vec::new();
    for (node, slot) in nodes.into_iter().zip(slots) {
        let wanted = wanted.clone();
        let stream = pw::stream::Stream::new(&core, "elo-tela", pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        })
        .map_err(|e| e.to_string())?;
        let listener = stream
            .add_local_listener_with_user_data(VideoInfoRaw::default())
            .param_changed(|_, info, id, param| {
                if let Some(param) = param
                    && id == pw::spa::param::ParamType::Format.as_raw()
                {
                    let _ = info.parse(param);
                }
            })
            .process(move |stream, info| {
                let Some(mut buffer) = stream.dequeue_buffer() else { return };
                if !wanted.load(Ordering::Relaxed) {
                    return;
                }
                let datas = buffer.datas_mut();
                let Some(d) = datas.first_mut() else { return };
                let (w, h) = (info.size().width, info.size().height);
                let stride = d.chunk().stride().max(0) as usize;
                let offset = d.chunk().offset() as usize;
                let Some(src) = d.data() else { return };
                if w == 0 || h == 0 || stride < w as usize * 4 || src.len() < offset + stride * h as usize {
                    return;
                }
                let src = &src[offset..];
                // Posição de R, G e B em cada pixel de 4 bytes.
                let (r, g, b) = match info.format() {
                    VideoFormat::BGRx | VideoFormat::BGRA => (2, 1, 0),
                    VideoFormat::RGBx | VideoFormat::RGBA => (0, 1, 2),
                    VideoFormat::xRGB | VideoFormat::ARGB => (1, 2, 3),
                    _ => return,
                };
                let mut rgba = vec![255u8; w as usize * h as usize * 4];
                for (y, row) in rgba.chunks_exact_mut(w as usize * 4).enumerate() {
                    let line = &src[y * stride..y * stride + w as usize * 4];
                    for (o, i) in row.as_chunks_mut::<4>().0.iter_mut().zip(line.as_chunks::<4>().0) {
                        o[0] = i[r];
                        o[1] = i[g];
                        o[2] = i[b];
                    }
                }
                if let Ok(mut s) = slot.lock() {
                    *s = Some(Frame { width: w, height: h, rgba });
                }
            })
            .register()
            .map_err(|e| e.to_string())?;
        let mut params = [Pod::from_bytes(&values).ok_or("formato inválido")?];
        stream.connect(spa::utils::Direction::Input, Some(node), pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS, &mut params).map_err(|e| e.to_string())?;
        keep.push((stream, listener));
    }
    mainloop.run();
    Ok(())
}

fn portal() -> Result<Arc<Portal>, String> {
    let mut cur = CURRENT.lock().map_err(|e| e.to_string())?;
    let p = match cur.upgrade() {
        Some(p) => p,
        None => {
            let p = start()?;
            *cur = Arc::downgrade(&p);
            p
        }
    };
    p.wanted.store(true, Ordering::Relaxed);
    // Segura enquanto houver acesso (o prazo conta a partir do fim).
    if let Ok(mut k) = KEEP.lock() {
        *k = Some((p.clone(), Instant::now() + Duration::from_secs(365 * 86_400)));
    }
    Ok(p)
}

/// Fim de um acesso: a sessão fica aberta por `KEEP_FOR` e depois fecha.
fn release() {
    if let Ok(mut k) = KEEP.lock()
        && let Some((p, _)) = k.take()
    {
        p.wanted.store(false, Ordering::Relaxed);
        let until = Instant::now() + KEEP_FOR;
        *k = Some((p, until));
        drop(k);
        let _ = std::thread::Builder::new().name("elo-portal-fim".into()).spawn(move || {
            std::thread::sleep(KEEP_FOR + Duration::from_secs(1));
            if let Ok(mut k) = KEEP.lock()
                && k.as_ref().is_some_and(|(_, t)| *t <= Instant::now())
            {
                *k = None;
            }
        });
    }
}

struct WaylandScreen {
    portal: Arc<Portal>,
    last: Vec<Option<Frame>>,
}

impl Drop for WaylandScreen {
    fn drop(&mut self) {
        release();
    }
}

impl Screen for WaylandScreen {
    fn monitors(&mut self) -> Vec<Monitor> {
        self.portal
            .streams
            .iter()
            .enumerate()
            .map(|(i, s)| Monitor { index: i as u8, name: format!("Tela {}", i + 1), width: s.width, height: s.height, primary: i == 0 })
            .collect()
    }

    fn capture(&mut self, monitor: u8) -> Result<Frame, String> {
        let i = (monitor as usize).min(self.portal.frames.len().saturating_sub(1));
        // O PipeWire só manda quadro quando a tela muda: guarda o último.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(f) = self.portal.frames[i].lock().map_err(|e| e.to_string())?.take() {
                self.last[i] = Some(f);
            }
            if let Some(f) = &self.last[i] {
                return Ok(Frame { width: f.width, height: f.height, rgba: f.rgba.clone() });
            }
            if Instant::now() > deadline {
                return Err("a tela compartilhada pelo portal não mandou imagem".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn cursor(&mut self, _monitor: u8) -> Option<(i32, i32)> {
        // O cursor já vem desenhado na imagem.
        None
    }
}

struct WaylandControl {
    portal: Arc<Portal>,
}

impl WaylandControl {
    fn send(&self, c: Cmd) {
        let _ = self.portal.cmd.send(c);
    }

    fn key(&self, sym: u32, down: bool) {
        self.send(Cmd::Keysym(sym as i32, down));
    }
}

impl Control for WaylandControl {
    fn apply(&mut self, input: &Input, monitor: u8) {
        let node = self.portal.streams.get(monitor as usize).or(self.portal.streams.first()).map(|s| s.node).unwrap_or(0);
        match input {
            Input::MouseMove { x, y } => self.send(Cmd::Motion(node, *x as f64, *y as f64)),
            Input::MouseButton { button, down } => {
                // Códigos do Linux (linux/input-event-codes.h): BTN_LEFT, BTN_RIGHT, BTN_MIDDLE.
                let b = match button {
                    Button::Left => 0x110,
                    Button::Right => 0x111,
                    Button::Middle => 0x112,
                };
                self.send(Cmd::Button(b, *down));
            }
            // No Wayland, positivo = para baixo/direita (aqui, positivo = cima/esquerda).
            Input::Scroll { dx, dy } => {
                if *dy != 0 {
                    self.send(Cmd::Discrete(Axis::Vertical, -dy));
                }
                if *dx != 0 {
                    self.send(Cmd::Discrete(Axis::Horizontal, -dx));
                }
            }
            // 1/120 de clique → pixels (um clique ≈ 15 px).
            Input::ScrollFine { dx, dy } => self.send(Cmd::Axis(-*dx as f64 / 8.0, -*dy as f64 / 8.0)),
            Input::Key { key, down } => self.key(crate::platform::keysym_of(*key), *down),
            Input::Text(t) => {
                for c in t.chars() {
                    let k = crate::platform::char_keysym(c);
                    self.key(k, true);
                    self.key(k, false);
                }
            }
            Input::SecureAttention => {
                let keys = [Key::Control, Key::Alt, Key::Delete];
                for k in keys {
                    self.key(crate::platform::keysym_of(k), true);
                }
                for k in keys.iter().rev() {
                    self.key(crate::platform::keysym_of(*k), false);
                }
            }
        }
    }
}

pub fn open_screen() -> Result<Box<dyn Screen>, String> {
    let portal = portal()?;
    let last = portal.streams.iter().map(|_| None).collect();
    Ok(Box::new(WaylandScreen { portal, last }))
}

pub fn open_control() -> Result<Box<dyn Control>, String> {
    Ok(Box::new(WaylandControl { portal: portal()? }))
}
