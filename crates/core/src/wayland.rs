// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Wayland (GNOME, KDE e outros): captura e controle pelo portal do sistema
//! (xdg-desktop-portal: RemoteDesktop + ScreenCast) e os quadros pelo PipeWire.
//!
//! No GNOME, o Elo fala direto com o Mutter (ver `mutter`) e nada é
//! perguntado na tela. Nos outros (KDE...), vai pelo portal: na primeira vez
//! o sistema pergunta na tela do computador acessado se pode compartilhar; o
//! Elo guarda a permissão (restore token) e, nas próximas, o portal não
//! pergunta de novo (até alguém revogar).

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

/// Um monitor compartilhado (pelo portal ou pelo Mutter).
pub(crate) struct StreamMeta {
    pub(crate) node: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// No GNOME o compartilhamento não precisa de ninguém na tela (vai pelo Mutter).
pub fn gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|x| x.eq_ignore_ascii_case("gnome")))
}

/// Alguém pode precisar aceitar o compartilhamento na tela deste computador
/// (Wayland fora do GNOME, até a primeira permissão ficar guardada).
pub fn screen_needs_consent() -> bool {
    is_wayland() && !gnome()
}

enum Cmd {
    /// Tela (índice), x, y.
    Motion(usize, f64, f64),
    Button(i32, bool),
    Axis(f64, f64),
    Discrete(Axis, i32),
    Keysym(i32, bool),
    /// Espera um pouco antes do próximo comando (ver `PAUSE`).
    Pause(Duration),
    /// Testes: pede ao sistema para encerrar a sessão, como o GNOME faz ao
    /// bloquear a tela (o Elo só fica sabendo pelo aviso de fechamento).
    StopForTest,
}

/// Último quadro de um monitor e o número dele (sobe a cada quadro novo).
/// Cada acesso lembra o número que já viu: vários acessos ao mesmo tempo leem
/// o mesmo quadro sem tirá-lo um do outro.
type Slot = Arc<Mutex<(u64, Option<Arc<Frame>>)>>;

/// Sessão do portal (compartilhada entre a captura e o controle).
struct Portal {
    streams: Vec<StreamMeta>,
    /// Último quadro de cada monitor.
    frames: Vec<Slot>,
    /// Sessão do Mutter (GNOME): abre na hora e sem perguntar, então não
    /// fica aberta depois do último acesso (o GNOME mostra no alto da tela
    /// que ela está sendo compartilhada).
    mutter: bool,
    /// Acessos capturando desta sessão agora (ver `Use`).
    users: AtomicUsize,
    /// Comandos com a época em que foram pedidos (ver `epoch`).
    cmd: tokio::sync::mpsc::UnboundedSender<(u64, Cmd)>,
    /// Sobe no fim da sessão: o que ainda estava na fila é descartado.
    epoch: Arc<AtomicU64>,
    /// Comandos na fila, para não acumular texto sem limite.
    pending: Arc<AtomicUsize>,
    quit_pw: Mutex<Option<pipewire::channel::Sender<()>>>,
    stop: Arc<AtomicBool>,
    /// Alguém está capturando (`users > 0`; sem ninguém, os quadros quase
    /// todos são descartados).
    wanted: Arc<AtomicBool>,
    /// Cai quando o sistema fecha a sessão por conta própria (no GNOME:
    /// tela bloqueada, monitor que some e volta). Aí a captura abre outra.
    alive: Arc<AtomicBool>,
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

/// Quem controla e captura: o portal ou o Mutter (GNOME).
enum Backend {
    Portal(RemoteDesktop, Session<RemoteDesktop>),
    Mutter(crate::mutter::Mutter),
}

impl Backend {
    /// Termina quando o sistema fechar a sessão (só o Mutter avisa; no portal,
    /// a queda aparece pelo PipeWire).
    async fn closed(&self) {
        match self {
            Backend::Mutter(m) => m.closed().await,
            Backend::Portal(..) => std::future::pending().await,
        }
    }

    async fn close(&self) {
        match self {
            Backend::Portal(_, s) => {
                let _ = s.close().await;
            }
            Backend::Mutter(m) => m.close().await,
        }
    }
}

/// GNOME: direto pelo Mutter, sem perguntar na tela. Se não der (ou fora do
/// GNOME), pelo portal.
async fn open_any() -> Result<(Backend, Vec<StreamMeta>, Option<OwnedFd>), String> {
    if gnome()
        && let Ok(conn) = ashpd::zbus::Connection::session().await
        && crate::mutter::present(&conn).await
    {
        match crate::mutter::open(&conn).await {
            Ok((m, streams)) => return Ok((Backend::Mutter(m), streams, None)),
            // O portal também seria recusado (e ficaria esperando um aceite).
            Err(e) if e == LOCKED => return Err(e),
            Err(e) => eprintln!("[gnome] {e}; tentando pelo portal"),
        }
    }
    let (rd, session, streams, fd) = open().await?;
    Ok((Backend::Portal(rd, session), streams, Some(fd)))
}

/// Erro de entrada que quer dizer "essa sessão não existe mais".
fn gone(e: &str) -> bool {
    e.contains("UnknownMethod") || e.contains("UnknownObject") || e.contains("não existe") || e.contains("does not exist") || e.contains("doesn't exist")
}

/// Manda a entrada; `false` se a sessão já não existe.
async fn apply(b: &Backend, nodes: &[u32], c: Cmd) -> bool {
    if matches!(c, Cmd::StopForTest) {
        b.close().await;
        return true;
    }
    if let Cmd::Pause(d) = c {
        tokio::time::sleep(d).await;
        return true;
    }
    let r: Result<(), String> = match b {
        Backend::Portal(rd, s) => {
            let e = |r: Result<(), ashpd::Error>| r.map_err(|e| e.to_string());
            match c {
                Cmd::Motion(i, x, y) => e(rd.notify_pointer_motion_absolute(s, nodes.get(i).or(nodes.first()).copied().unwrap_or(0), x, y, Default::default()).await),
                Cmd::Button(b, down) => e(rd.notify_pointer_button(s, b, if down { KeyState::Pressed } else { KeyState::Released }, Default::default()).await),
                Cmd::Axis(dx, dy) => e(rd.notify_pointer_axis(s, dx, dy, Default::default()).await),
                Cmd::Discrete(axis, steps) => e(rd.notify_pointer_axis_discrete(s, axis, steps, Default::default()).await),
                Cmd::Keysym(k, down) => e(rd.notify_keyboard_keysym(s, k, if down { KeyState::Pressed } else { KeyState::Released }, Default::default()).await),
                Cmd::StopForTest | Cmd::Pause(_) => Ok(()),
            }
        }
        Backend::Mutter(m) => match c {
            Cmd::Motion(i, x, y) => m.motion(i, x, y).await,
            Cmd::Button(b, down) => m.button(b, down).await,
            Cmd::Axis(dx, dy) => m.axis(dx, dy).await,
            Cmd::Discrete(axis, steps) => m.discrete(if matches!(axis, Axis::Horizontal) { 1 } else { 0 }, steps).await,
            Cmd::Keysym(k, down) => m.keysym(k as u32, down).await,
            Cmd::StopForTest | Cmd::Pause(_) => Ok(()),
        },
    };
    // Raro (ex.: o usuário revogou o controle); um aviso por tipo de erro basta.
    if let Err(e) = r {
        static AVISADO: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let m = e.to_string();
        let dead = gone(&m);
        if let Ok(mut v) = AVISADO.lock()
            && !v.contains(&m)
        {
            eprintln!("[portal] entrada recusada: {m}");
            v.push(m);
        }
        return !dead;
    }
    true
}

fn start() -> Result<Arc<Portal>, String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(Vec<StreamMeta>, Option<OwnedFd>), String>>();
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Cmd)>();
    let epoch = Arc::new(AtomicU64::new(0));
    let pending = Arc::new(AtomicUsize::new(0));
    let (run_epoch, run_pending) = (epoch.clone(), pending.clone());
    let alive = Arc::new(AtomicBool::new(true));
    let run_alive = alive.clone();
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
                match open_any().await {
                    Ok((backend, streams, fd)) => {
                        let nodes: Vec<u32> = streams.iter().map(|s| s.node).collect();
                        let _ = ready_tx.send(Ok((streams, fd)));
                        // Entradas até a sessão acabar (o `Portal` é descartado)
                        // ou o sistema fechá-la por conta própria.
                        let closed = backend.closed();
                        tokio::pin!(closed);
                        loop {
                            tokio::select! {
                                m = cmd_rx.recv() => {
                                    let Some((e, c)) = m else { break };
                                    run_pending.fetch_sub(1, Ordering::Relaxed);
                                    if e == run_epoch.load(Ordering::Relaxed) && !apply(&backend, &nodes, c).await {
                                        eprintln!("[portal] a sessão de acesso remoto não existe mais; abrindo outra");
                                        run_alive.store(false, Ordering::Relaxed);
                                        break;
                                    }
                                }
                                _ = &mut closed => {
                                    eprintln!("[gnome] abrindo outra sessão de acesso remoto");
                                    run_alive.store(false, Ordering::Relaxed);
                                    break;
                                }
                            }
                        }
                        backend.close().await;
                        eprintln!("[portal] sessão fechada");
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        })
        .map_err(|e| e.to_string())?;
    // Portal: espera alguém aceitar na tela (menos que os 95 s de quem acessa,
    // para o aviso chegar lá). O Mutter responde na hora.
    let (streams, fd) = ready_rx.recv_timeout(Duration::from_secs(75)).map_err(|_| NOBODY_ANSWERED.to_string())??;
    let frames: Vec<Slot> = streams.iter().map(|_| Arc::new(Mutex::new((0, None)))).collect();
    // Só o Mutter não passa o acesso ao PipeWire (usa o da sessão).
    let mutter = fd.is_none();
    let stop = Arc::new(AtomicBool::new(false));
    let wanted = Arc::new(AtomicBool::new(false));
    let (quit_tx, quit_rx) = pipewire::channel::channel::<()>();
    let nodes: Vec<u32> = streams.iter().map(|s| s.node).collect();
    let slots = frames.clone();
    let w = wanted.clone();
    let pw_alive = alive.clone();
    let pw_stop = stop.clone();
    std::thread::Builder::new()
        .name("elo-pipewire".into())
        .spawn(move || {
            if let Err(e) = pipewire_loop(fd, nodes, slots, w, pw_alive.clone(), pw_stop, quit_rx) {
                eprintln!("PipeWire: {e}");
                pw_alive.store(false, Ordering::Relaxed);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(Portal { streams, frames, mutter, users: AtomicUsize::new(0), cmd: cmd_tx, epoch, pending, quit_pw: Mutex::new(Some(quit_tx)), stop, wanted, alive }))
}

/// Recebe os quadros de cada monitor e guarda o último, em RGBA.
fn pipewire_loop(fd: Option<OwnedFd>, nodes: Vec<u32>, slots: Vec<Slot>, wanted: Arc<AtomicBool>, alive: Arc<AtomicBool>, stop: Arc<AtomicBool>, quit: pipewire::channel::Receiver<()>) -> Result<(), String> {
    use pipewire as pw;
    use pw::spa;
    use spa::param::video::{VideoFormat, VideoInfoRaw};
    use spa::pod::Pod;

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| e.to_string())?;
    // Portal: o PipeWire liberado por ele; Mutter: o PipeWire da sessão.
    let core = match fd {
        Some(fd) => context.connect_fd_rc(fd, None),
        None => context.connect_rc(None),
    }
    .map_err(|e| e.to_string())?;
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
            // O Elo manda no máximo 24 por segundo: mais que isso só gasta
            // processador convertendo quadros que ninguém vai ver.
            spa::utils::Fraction { num: 30, denom: 1 }
        ),
    );
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &pw::spa::pod::Value::Object(obj)).map_err(|e| format!("{e:?}"))?.0.into_inner();

    let mut keep = Vec::new();
    for (node, slot) in nodes.into_iter().zip(slots) {
        let wanted = wanted.clone();
        let alive = alive.clone();
        let stop = stop.clone();
        // Sem ninguém capturando, guarda um quadro por segundo: quem chegar
        // tem uma imagem recente mesmo com a tela parada (o PipeWire só manda
        // quadro quando ela muda).
        let mut kept = Instant::now() - Duration::from_secs(10);
        let stream = pw::stream::StreamRc::new(core.clone(), "elo-tela", pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        })
        .map_err(|e| e.to_string())?;
        let listener = stream
            .add_local_listener_with_user_data(VideoInfoRaw::default())
            // O sistema derrubou o fluxo (sessão fechada, monitor que sumiu).
            .state_changed(move |_, _, old, new| {
                let dead = matches!(new, pw::stream::StreamState::Error(_)) || (matches!(new, pw::stream::StreamState::Unconnected) && !matches!(old, pw::stream::StreamState::Connecting));
                // (No fim normal da sessão os fluxos também desconectam.)
                if dead && !stop.load(Ordering::Relaxed) {
                    eprintln!("[pipewire] a imagem de uma tela parou ({old:?} → {new:?})");
                    alive.store(false, Ordering::Relaxed);
                }
            })
            .param_changed(|_, info, id, param| {
                if let Some(param) = param
                    && id == pw::spa::param::ParamType::Format.as_raw()
                {
                    let _ = info.parse(param);
                }
            })
            .process(move |stream, info| {
                let Some(mut buffer) = stream.dequeue_buffer() else { return };
                if !wanted.load(Ordering::Relaxed) && kept.elapsed() < Duration::from_secs(1) {
                    return;
                }
                kept = Instant::now();
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
                    *s = (s.0 + 1, Some(Arc::new(Frame { width: w, height: h, rgba })));
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

/// Já há um pedido de compartilhar a tela esperando alguém responder.
static STARTING: AtomicBool = AtomicBool::new(false);

/// O pedido que está esperando resposta na tela (só o portal pergunta).
pub const ALREADY_ASKING: &str = "este computador já está esperando alguém liberar o compartilhamento da tela (uma janela do sistema na tela dele): peça para clicarem em Compartilhar lá";

fn portal() -> Result<Arc<Portal>, String> {
    // Não segura a trava enquanto espera a resposta na tela: antes, um pedido
    // sem resposta prendia em silêncio todas as tentativas seguintes.
    // Outro acesso abrindo ao mesmo tempo: espera ele (no GNOME é na hora) e
    // usa a mesma sessão. O prazo é o mesmo da resposta na tela.
    let since = Instant::now();
    let p = loop {
        // (Solta a trava antes: abaixo ela é pega de novo.)
        let cur = CURRENT.lock().map_err(|e| e.to_string())?.upgrade();
        if let Some(p) = cur {
            if p.alive.load(Ordering::Relaxed) {
                break p;
            }
            // Morta: não serve nem para reaproveitar depois de um acesso.
            *CURRENT.lock().map_err(|e| e.to_string())? = Weak::new();
            if let Ok(mut k) = KEEP.lock()
                && k.as_ref().is_some_and(|(kp, _)| Arc::ptr_eq(kp, &p))
            {
                *k = None;
            }
            continue;
        }
        if !STARTING.swap(true, Ordering::SeqCst) {
            let r = start();
            let p = match r {
                Ok(p) => p,
                Err(e) => {
                    STARTING.store(false, Ordering::SeqCst);
                    return Err(e);
                }
            };
            *CURRENT.lock().map_err(|e| e.to_string())? = Arc::downgrade(&p);
            STARTING.store(false, Ordering::SeqCst);
            break p;
        }
        if since.elapsed() > Duration::from_secs(80) {
            return Err(ALREADY_ASKING.into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    Ok(p)
}

/// Um acesso capturando de uma sessão. Enquanto houver algum, os quadros
/// chegam; quando o último acaba, a sessão do portal fica aberta por
/// `KEEP_FOR` (para não perguntar de novo a quem reconectar) e a do Mutter
/// fecha na hora.
struct Use(Arc<Portal>);

impl Use {
    fn new(p: Arc<Portal>) -> Self {
        p.users.fetch_add(1, Ordering::SeqCst);
        p.wanted.store(true, Ordering::Relaxed);
        // Segura enquanto houver acesso (o prazo conta a partir do fim).
        if let Ok(mut k) = KEEP.lock()
            && !k.as_ref().is_some_and(|(kp, _)| Arc::ptr_eq(kp, &p))
        {
            *k = Some((p.clone(), Instant::now() + Duration::from_secs(365 * 86_400)));
        }
        Use(p)
    }
}

impl Drop for Use {
    fn drop(&mut self) {
        let p = &self.0;
        if p.users.fetch_sub(1, Ordering::SeqCst) != 1 {
            return;
        }
        p.wanted.store(false, Ordering::Relaxed);
        let Ok(mut k) = KEEP.lock() else { return };
        if !k.as_ref().is_some_and(|(kp, _)| Arc::ptr_eq(kp, p)) {
            return;
        }
        if p.mutter || !p.alive.load(Ordering::Relaxed) {
            *k = None;
            return;
        }
        let until = Instant::now() + KEEP_FOR;
        *k = Some((p.clone(), until));
        drop(k);
        let _ = std::thread::Builder::new().name("elo-portal-fim".into()).spawn(move || {
            std::thread::sleep(KEEP_FOR + Duration::from_secs(1));
            if let Ok(mut k) = KEEP.lock()
                && k.as_ref().is_some_and(|(kp, t)| *t <= Instant::now() && kp.users.load(Ordering::SeqCst) == 0)
            {
                *k = None;
            }
        });
    }
}

/// Erro da captura com a tela bloqueada (o GNOME não deixa capturar).
pub const LOCKED: &str = "a tela está bloqueada e o GNOME não deixa capturá-la";

/// Monitores enquanto a tela está bloqueada (sem sessão de captura).
fn locked_monitors() -> Vec<Monitor> {
    let r = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string()).and_then(|rt| {
        rt.block_on(async {
            let conn = ashpd::zbus::Connection::session().await.map_err(|e| e.to_string())?;
            crate::mutter::monitors(&conn).await
        })
    });
    let list = r.ok().filter(|l| !l.is_empty()).unwrap_or_else(|| vec![(1920, 1080, true)]);
    list.into_iter().enumerate().map(|(i, (width, height, primary))| Monitor { index: i as u8, name: format!("Tela {}", i + 1), width, height, primary }).collect()
}

struct WaylandScreen {
    /// `None`: a tela estava bloqueada ao conectar; abre ao desbloquear.
    portal: Option<Use>,
    /// Lista de telas quando não há sessão aberta.
    locked: Vec<Monitor>,
    /// Último quadro visto de cada tela e o número dele.
    last: Vec<(u64, Option<Arc<Frame>>)>,
    /// Próxima tentativa de abrir outra sessão quando a atual morre.
    retry_at: Instant,
    /// A última tentativa foi recusada porque a tela está bloqueada.
    blocked: bool,
}

/// Sem sessão com o sistema (fechada por ele): a captura tenta de novo.
const GONE: &str = "o sistema fechou o compartilhamento da tela; abrindo de novo";

impl Screen for WaylandScreen {
    fn monitors(&mut self) -> Vec<Monitor> {
        let Some(Use(portal)) = &self.portal else { return self.locked.clone() };
        portal
            .streams
            .iter()
            .enumerate()
            .map(|(i, s)| Monitor { index: i as u8, name: format!("Tela {}", i + 1), width: s.width, height: s.height, primary: i == 0 })
            .collect()
    }

    fn capture(&mut self, monitor: u8) -> Result<Frame, String> {
        if !self.portal.as_ref().is_some_and(|p| p.0.alive.load(Ordering::Relaxed)) {
            // Uma tentativa a cada 2 s (no GNOME é na hora; o portal pergunta).
            if Instant::now() < self.retry_at {
                return Err(if self.blocked { LOCKED.into() } else { GONE.into() });
            }
            self.retry_at = Instant::now() + Duration::from_secs(2);
            let p = portal().inspect_err(|e| self.blocked = e == LOCKED)?;
            self.blocked = false;
            self.last = p.streams.iter().map(|_| (0, None)).collect();
            self.portal = Some(Use::new(p));
        }
        let Some(Use(portal)) = &self.portal else { return Err(GONE.into()) };
        let i = (monitor as usize).min(portal.frames.len().saturating_sub(1));
        // O PipeWire só manda quadro quando a tela muda: guarda o último.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            {
                let slot = portal.frames[i].lock().map_err(|e| e.to_string())?;
                if slot.0 != self.last[i].0 {
                    self.last[i] = (slot.0, slot.1.clone());
                }
            }
            if let Some(f) = &self.last[i].1 {
                return Ok(Frame { width: f.width, height: f.height, rgba: f.rgba.clone() });
            }
            if Instant::now() > deadline || !portal.alive.load(Ordering::Relaxed) {
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

/// Ninguém respondeu à janela do sistema que pede para compartilhar a tela.
pub const NOBODY_ANSWERED: &str = "o outro computador (Linux com Wayland) pediu na tela dele para liberar o compartilhamento e ninguém respondeu. Alguém precisa clicar em Compartilhar lá; para acessar sem ninguém por perto, entre lá na sessão Xorg";

/// Intervalo entre as letras de um texto com tecla morta, e antes e depois
/// da letra acentuada (ver `Input::Text`).
const PAUSE: Duration = Duration::from_millis(12);
const PAUSE_DEAD: Duration = Duration::from_millis(40);

/// Acima disto na fila, texto novo é ignorado.
const MAX_QUEUED: usize = 8192;

struct WaylandControl {
    /// `None` enquanto a tela está bloqueada (a captura abre ao desbloquear).
    portal: Option<Arc<Portal>>,
    /// Mapa de teclado daqui e quando foi lido (relido de vez em quando: a
    /// pessoa pode trocar o teclado).
    keymap: Option<(Instant, Option<crate::keymap::Keymap>)>,
}

impl WaylandControl {
    fn send(&mut self, c: Cmd) {
        // A sessão morreu e a captura já abriu outra: passa a usar a nova.
        if !self.portal.as_ref().is_some_and(|p| p.alive.load(Ordering::Relaxed))
            && let Some(p) = CURRENT.lock().ok().and_then(|c| c.upgrade()).filter(|p| p.alive.load(Ordering::Relaxed))
        {
            self.portal = Some(p);
        }
        let Some(p) = &self.portal else { return };
        p.pending.fetch_add(1, Ordering::Relaxed);
        let _ = p.cmd.send((p.epoch.load(Ordering::Relaxed), c));
    }

    fn key(&mut self, sym: u32, down: bool) {
        self.send(Cmd::Keysym(sym as i32, down));
    }

    fn keymap(&mut self) -> Option<&crate::keymap::Keymap> {
        if !self.keymap.as_ref().is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(10)) {
            self.keymap = Some((Instant::now(), crate::keymap::Keymap::current()));
        }
        self.keymap.as_ref().and_then(|(_, k)| k.as_ref())
    }
}

impl Control for WaylandControl {
    fn discard_pending(&mut self) {
        if let Some(p) = &self.portal {
            p.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn apply(&mut self, input: &Input, monitor: u8) {
        let screens = self.portal.as_ref().map_or(1, |p| p.streams.len());
        let screen = (monitor as usize).min(screens.saturating_sub(1));
        match input {
            Input::MouseMove { x, y } => self.send(Cmd::Motion(screen, *x as f64, *y as f64)),
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
                if self.portal.as_ref().is_some_and(|p| p.pending.load(Ordering::Relaxed) > MAX_QUEUED) {
                    return;
                }
                // Letra que não existe direto no teclado daqui: tecla morta +
                // letra (senão o sistema descarta).
                let plan: Vec<Vec<u32>> = t.chars().map(|c| self.keymap().and_then(|k| k.plan(c)).unwrap_or_else(|| vec![crate::platform::char_keysym(c)])).collect();
                // Com tecla morta no meio, tudo vai com intervalo: a janela que
                // recebe compõe a letra acentuada à parte e, sem folga, troca a
                // ordem com as teclas vizinhas ("você" saía "vêoc").
                let paced = plan.iter().any(|k| k.len() > 1);
                for keys in plan {
                    let dead = keys.len() > 1;
                    if dead {
                        self.send(Cmd::Pause(PAUSE_DEAD));
                    }
                    for k in keys {
                        self.key(k, true);
                        self.key(k, false);
                    }
                    if dead {
                        self.send(Cmd::Pause(PAUSE_DEAD));
                    } else if paced {
                        self.send(Cmd::Pause(PAUSE));
                    }
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

/// Testes: o sistema encerra a sessão atual (como o GNOME ao bloquear a tela).
#[doc(hidden)]
pub fn stop_session_for_test() {
    if let Some(p) = CURRENT.lock().ok().and_then(|c| c.upgrade()) {
        p.pending.fetch_add(1, Ordering::Relaxed);
        let _ = p.cmd.send((p.epoch.load(Ordering::Relaxed), Cmd::StopForTest));
    }
}

/// Testes: há uma sessão de captura aberta com o sistema?
#[doc(hidden)]
pub fn session_open_for_test() -> bool {
    CURRENT.lock().ok().and_then(|c| c.upgrade()).is_some()
}

pub fn open_screen() -> Result<Box<dyn Screen>, String> {
    match portal() {
        Ok(portal) => {
            let last = portal.streams.iter().map(|_| (0, None)).collect();
            Ok(Box::new(WaylandScreen { portal: Some(Use::new(portal)), locked: Vec::new(), last, retry_at: Instant::now(), blocked: false }))
        }
        // Tela bloqueada: a sessão começa sem imagem (quem acessa pode
        // desbloquear com a senha de lá) e a captura abre depois.
        Err(e) if e == LOCKED => {
            let locked = locked_monitors();
            Ok(Box::new(WaylandScreen { portal: None, last: locked.iter().map(|_| (0, None)).collect(), locked, retry_at: Instant::now(), blocked: true }))
        }
        Err(e) => Err(e),
    }
}

pub fn open_control() -> Result<Box<dyn Control>, String> {
    match portal() {
        Ok(p) => Ok(Box::new(WaylandControl { portal: Some(p), keymap: None })),
        Err(e) if e == LOCKED => Ok(Box::new(WaylandControl { portal: None, keymap: None })),
        Err(e) => Err(e),
    }
}
