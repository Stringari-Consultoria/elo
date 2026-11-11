//! Ponta a ponta: servidor real (sem TLS), anfitrião e visualizador pelo relay.
//! A captura usa o X11 de `DISPLAY`; sem tela, o teste é pulado.

use std::process::{Child, Command};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elo_core::client::{self, Event};
use elo_core::net::ServerCfg;
use elo_core::session::{self, HostEvent, HostOptions, Remote, ViewerEvent};

/// Reinício de mentira: marca que foi pedido (o teste não reinicia a máquina).
static REINICIOS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn restart_simulado() -> Result<(), String> {
    REINICIOS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Terminal de mentira: conta os pedidos (o teste não abre janelas).
static TERMINAIS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn terminal_simulado() -> Result<(), String> {
    TERMINAIS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn wait<T>(rx: &std::sync::mpsc::Receiver<T>, secs: u64, mut f: impl FnMut(&T) -> bool) -> Option<T> {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(100))
            && f(&e)
        {
            return Some(e);
        }
    }
    None
}

/// Conexão de controle crua, sem TLS: `Hello` com o nome dado.
fn raw_hello(port: u16, token: u8, name: &str) -> std::net::TcpStream {
    let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let hello = elo_proto::ctrl::ToServer::Hello { token: [token; 32], name: name.into(), os: "Linux".into(), version: elo_proto::PROTOCOL_VERSION, mac: String::new() };
    elo_proto::frame::send(&mut c, &hello).unwrap();
    c
}

#[test]
fn flood_of_control_messages_loses_the_connection() {
    let port = 29000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-inundacao-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    // Nome enorme com quebra de linha: guardado curto e sem o controle.
    let enorme = format!("a\nb{}", "x".repeat(60_000));
    let mut c = raw_hello(port, 9, &enorme);
    std::thread::sleep(Duration::from_millis(300));
    let saved = std::fs::read_to_string(dir.join("dispositivos.json")).unwrap();
    assert!(saved.len() < 2000 && !saved.contains("a\\nb"), "{}", &saved[..saved.len().min(200)]);
    // Manda sem ler as respostas: o servidor corta em vez de acumular.
    c.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut cut = false;
    for _ in 0..50_000 {
        if elo_proto::frame::send(&mut c, &elo_proto::ctrl::ToServer::Ping).is_err() {
            cut = true;
            break;
        }
    }
    assert!(cut, "o servidor continuou aceitando a inundação");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_through_relay() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("sem DISPLAY: teste pulado");
        return;
    }
    // Os dois lados no mesmo computador: sem som (faria eco).
    session::AUDIO_ON_START.store(false, std::sync::atomic::Ordering::Relaxed);
    let port = 22000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-{port}"));
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let cfg = ServerCfg { address: format!("127.0.0.1:{port}"), tls: false, pin: String::new() };
    let host = client::start(cfg.clone(), [1u8; 32], "Anfitrião".into());
    let viewer = client::start(cfg.clone(), [2u8; 32], "Visualizador".into());
    let Some(Event::Online { id: host_id }) = wait(&host.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("anfitrião offline") };
    let Some(Event::Online { id: viewer_id }) = wait(&viewer.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("visualizador offline") };
    assert_eq!(host_id.len(), 9);
    // ID estável para o mesmo segredo.
    let again = client::start(cfg.clone(), [1u8; 32], "Anfitrião".into());
    let Some(Event::Online { id: same }) = wait(&again.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!() };
    assert_eq!(same, host_id);
    again.tx.send(client::Command::Quit).unwrap();
    std::thread::sleep(Duration::from_millis(300));

    for (pw, ok) in [("errada", false), ("s3nha-temp", true)] {
        viewer.tx.send(client::Command::Connect { target: host_id.clone() }).unwrap();
        let Some(Event::SessionReady { session: vs, secret: vsec, .. }) = wait(&viewer.rx, 10, |e| matches!(e, Event::SessionReady { .. })) else { panic!("sem sessão") };
        // O convite chega ao anfitrião (pode ter ido para a conexão mais recente).
        let inc = wait(&host.rx, 5, |e| matches!(e, Event::Incoming { .. }));
        let (hs, hsec, from) = match inc {
            Some(Event::Incoming { session, secret, from_id, .. }) => (session, secret, from_id),
            _ => panic!("convite não chegou"),
        };
        assert_eq!(hs, vs);
        assert_eq!(from, viewer_id);
        let (htx, hrx) = channel();
        let opt = HostOptions { passwords: vec!["s3nha-temp".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default(), restart: restart_simulado, terminal: terminal_simulado, audio: true, id: host_id.clone(), tech_ca: None };
        let hh = session::host(cfg.clone(), hs, hsec, from.clone(), "Visualizador".into(), opt, htx);
        let remote = Arc::new(Mutex::new(Remote::default()));
        let (vtx, vrx) = channel();
        let vh = session::viewer(cfg.clone(), vs, vsec, pw.into(), viewer_id.clone(), "Visualizador".into(), dir.join("baixados"), remote.clone(), vtx, host_id.clone(), None);
        if !ok {
            let Some(ViewerEvent::Ended(r)) = wait(&vrx, 15, |e| matches!(e, ViewerEvent::Ended(_))) else { panic!() };
            assert!(r.contains("senha"), "{r}");
            assert!(wait(&hrx, 5, |e| matches!(e, HostEvent::AuthFailed { .. })).is_some());
            continue;
        }
        let Some(ViewerEvent::Connected { sas, monitors, .. }) = wait(&vrx, 15, |e| matches!(e, ViewerEvent::Connected { .. })) else { panic!("não conectou") };
        assert!(!monitors.is_empty());
        let Some(HostEvent::Started { sas: hsas, .. }) = wait(&hrx, 5, |e| matches!(e, HostEvent::Started { .. })) else { panic!() };
        assert_eq!(sas, hsas);
        // O anfitrião anuncia que sabe mandar som (lista de recursos depois do Welcome).
        assert!(wait(&vrx, 5, |e| matches!(e, ViewerEvent::Audio { available: true })).is_some(), "o som não foi anunciado");
        // Recebe a tela.
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let r = remote.lock().unwrap();
            if r.version > 0 && r.dec.w > 0 {
                break;
            }
            drop(r);
            assert!(Instant::now() < end, "nenhum quadro recebido");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Mouse remoto e chat.
        vh.cmd.send(session::Cmd::Input(elo_proto::msg::Input::MouseMove { x: 37, y: 41 })).unwrap();
        vh.cmd.send(session::Cmd::Chat("olá".into())).unwrap();
        assert!(wait(&hrx, 5, |e| matches!(e, HostEvent::Chat(t) if t == "olá")).is_some());
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let c = remote.lock().unwrap().cursor;
            if c == Some((37, 41)) {
                break;
            }
            assert!(Instant::now() < end, "o cursor não chegou a 37,41: {c:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Arquivo do visualizador para o anfitrião.
        let f = dir.join("teste.txt");
        std::fs::write(&f, vec![b'x'; 500_000]).unwrap();
        vh.cmd.send(session::Cmd::SendFile(f.clone())).unwrap();
        let Some(HostEvent::File(t)) = wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.finished)) else { panic!("arquivo não chegou") };
        assert_eq!(std::fs::metadata(t.path.unwrap()).unwrap().len(), 500_000);
        // Gerenciador de arquivos: lista a pasta, baixa um arquivo e envia para uma subpasta.
        let shared = dir.join("compartilhada");
        std::fs::create_dir_all(shared.join("sub")).unwrap();
        std::fs::write(shared.join("remoto.txt"), vec![b'r'; 1000]).unwrap();
        vh.cmd.send(session::Cmd::ListDir(shared.to_string_lossy().into_owned())).unwrap();
        let Some(ViewerEvent::Listing(l)) = wait(&vrx, 5, |e| matches!(e, ViewerEvent::Listing(_))) else { panic!("listagem não chegou") };
        assert!(l.error.is_none(), "{:?}", l.error);
        assert!(l.entries[0].dir && l.entries[0].name == "sub", "pastas primeiro: {:?}", l.entries);
        assert!(l.entries.iter().any(|e| e.name == "remoto.txt" && e.size == 1000));
        vh.cmd.send(session::Cmd::ListDir(shared.join("nao-existe").to_string_lossy().into_owned())).unwrap();
        let Some(ViewerEvent::Listing(l)) = wait(&vrx, 5, |e| matches!(e, ViewerEvent::Listing(_))) else { panic!() };
        assert!(l.error.is_some());
        // Unidades e discos: lista da raiz de cada um, sem pasta de cima.
        vh.cmd.send(session::Cmd::ListDir(elo_proto::msg::DRIVES.into())).unwrap();
        let Some(ViewerEvent::Listing(l)) = wait(&vrx, 5, |e| matches!(e, ViewerEvent::Listing(_))) else { panic!() };
        assert!(l.path == elo_proto::msg::DRIVES && l.parent.is_none() && l.error.is_none(), "{l:?}");
        assert!(!l.entries.is_empty() && l.entries.iter().all(|e| e.dir && std::path::Path::new(&e.name).is_absolute()), "{:?}", l.entries);
        vh.cmd.send(session::Cmd::Download(shared.join("remoto.txt").to_string_lossy().into_owned())).unwrap();
        let Some(ViewerEvent::File(t)) = wait(&vrx, 10, |e| matches!(e, ViewerEvent::File(t) if t.incoming && t.finished)) else { panic!("download não chegou") };
        assert_eq!(std::fs::read(t.path.unwrap()).unwrap(), vec![b'r'; 1000]);
        vh.cmd.send(session::Cmd::Upload(f.clone(), shared.join("sub").to_string_lossy().into_owned())).unwrap();
        let Some(HostEvent::File(t)) = wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.incoming && t.finished)) else { panic!("upload não chegou") };
        let up = t.path.unwrap();
        assert!(up.starts_with(shared.join("sub")), "{}", up.display());
        assert_eq!(std::fs::metadata(&up).unwrap().len(), 500_000);
        // Retirar "Arquivos" para o que já está em andamento, dos dois lados, e sem deixar parcial.
        let grande = dir.join("grande.bin");
        std::fs::write(&grande, vec![7u8; 64 * 1024 * 1024]).unwrap();
        let destino = shared.join("revogado");
        std::fs::create_dir_all(&destino).unwrap();
        vh.cmd.send(session::Cmd::Upload(grande, destino.to_string_lossy().into_owned())).unwrap();
        assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.incoming && t.done > 0)).is_some(), "o envio não começou");
        hh.cmd.send(session::Cmd::Permissions(elo_proto::msg::Permissions { files: false, ..Default::default() })).unwrap();
        assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.incoming && t.error.is_some())).is_some(), "o recebimento continuou depois de retirar a permissão");
        assert!(wait(&vrx, 10, |e| matches!(e, ViewerEvent::File(t) if !t.incoming && t.error.is_some())).is_some(), "quem enviava não soube");
        assert_eq!(std::fs::read_dir(&destino).unwrap().count(), 0, "ficou arquivo pela metade");
        hh.cmd.send(session::Cmd::Permissions(Default::default())).unwrap();
        // Terminal: só com permissão de controle, e quem está lá fica sabendo.
        hh.cmd.send(session::Cmd::Permissions(elo_proto::msg::Permissions { control: false, ..Default::default() })).unwrap();
        assert!(wait(&vrx, 10, |e| matches!(e, ViewerEvent::Permissions(p) if !p.control)).is_some());
        let antes = TERMINAIS.load(std::sync::atomic::Ordering::SeqCst);
        vh.cmd.send(session::Cmd::OpenTerminal).unwrap();
        assert!(wait(&vrx, 10, |e| matches!(e, ViewerEvent::Notice(t) if t.contains("terminal"))).is_some(), "sem controle, o terminal não foi recusado");
        assert_eq!(TERMINAIS.load(std::sync::atomic::Ordering::SeqCst), antes);
        hh.cmd.send(session::Cmd::Permissions(Default::default())).unwrap();
        assert!(wait(&vrx, 10, |e| matches!(e, ViewerEvent::Permissions(p) if p.control)).is_some());
        vh.cmd.send(session::Cmd::OpenTerminal).unwrap();
        assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::Chat(t) if t.contains("abriu o terminal"))).is_some(), "quem está lá não soube do terminal");
        assert_eq!(TERMINAIS.load(std::sync::atomic::Ordering::SeqCst), antes + 1);
        // Reiniciar: o anfitrião responde com o motivo combinado (o visualizador reconecta).
        let antes = REINICIOS.load(std::sync::atomic::Ordering::SeqCst);
        vh.cmd.send(session::Cmd::Restart).unwrap();
        let Some(ViewerEvent::Ended(r)) = wait(&vrx, 10, |e| matches!(e, ViewerEvent::Ended(_))) else { panic!("a sessão não terminou ao reiniciar") };
        assert_eq!(r, elo_proto::msg::BYE_RESTART);
        assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::Ended(_))).is_some());
        assert_eq!(REINICIOS.load(std::sync::atomic::Ordering::SeqCst), antes + 1);
        let _ = hh;
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tls_with_pinned_certificate() {
    use std::io::{BufRead, BufReader};
    let port = 23000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-tls-{port}"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--self-signed"]).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut fp = String::new();
    for _ in 0..5 {
        let mut l = String::new();
        err.read_line(&mut l).unwrap();
        if let Some(x) = l.split("apps: ").nth(1) {
            fp = x.trim().to_string();
            break;
        }
    }
    let _srv = Server(child);
    assert_eq!(fp.len(), 64, "impressão digital não encontrada");
    std::thread::sleep(Duration::from_millis(300));
    let good = client::start(ServerCfg { address: format!("127.0.0.1:{port}"), tls: true, pin: fp.clone() }, [3u8; 32], "A".into());
    assert!(wait(&good.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
    let bad = client::start(ServerCfg { address: format!("127.0.0.1:{port}"), tls: true, pin: "00".repeat(32) }, [4u8; 32], "B".into());
    let Some(Event::Offline { reason }) = wait(&bad.rx, 10, |e| matches!(e, Event::Offline { .. })) else { panic!() };
    assert!(reason.contains("TLS") || reason.contains("certificado"), "{reason}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Servidor embutido (como o app usa): sobe numa thread, o cliente conecta
/// fixando a impressão digital, e ao parar a porta fica livre para religar.
#[test]
fn background_server_restarts_on_same_port() {
    let dir = std::env::temp_dir().join(format!("elo-bg-{}", std::process::id()));
    let cfg = |listen: String| elo_server::Config { listen, data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90, trusted_proxies: Vec::new() };
    let bg = elo_server::Background::start(cfg("127.0.0.1:0".into())).unwrap();
    assert!(bg.running());
    let port = bg.local_addr.port();
    let fp = bg.fingerprint.clone().unwrap();
    assert_eq!(elo_server::self_signed_fingerprint(&dir).as_deref(), Some(fp.as_str()));
    let addr = format!("127.0.0.1:{port}");
    let c = client::start(ServerCfg { address: addr.clone(), tls: true, pin: fp.clone() }, [5u8; 32], "Local".into());
    let Some(Event::Online { id }) = wait(&c.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("não conectou ao servidor embutido") };
    // Porta ocupada: erro claro na hora de iniciar.
    assert!(elo_server::Background::start(cfg(addr.clone())).is_err());
    drop(bg);
    assert!(wait(&c.rx, 10, |e| matches!(e, Event::Offline { .. })).is_some(), "a conexão deveria cair ao parar o servidor");
    // Religa na mesma porta; mesmo certificado e mesmo ID.
    let bg = elo_server::Background::start(cfg(addr)).unwrap();
    assert_eq!(bg.fingerprint.as_deref(), Some(fp.as_str()));
    let Some(Event::Online { id: again }) = wait(&c.rx, 40, |e| matches!(e, Event::Online { .. })) else { panic!("não reconectou") };
    assert_eq!(again, id);
    c.tx.send(client::Command::Quit).unwrap();
    drop(bg);
    let _ = std::fs::remove_dir_all(dir);
}

/// Como fica atrás do Cloudflare Tunnel: o cloudflared (local) fala WebSocket
/// sem TLS com o servidor; um lado usa WebSocket e o outro o protocolo direto.
#[test]
fn websocket_like_cloudflare_tunnel() {
    let dir = std::env::temp_dir().join(format!("elo-ws-{}", std::process::id()));
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90, trusted_proxies: Vec::new() }).unwrap();
    let port = bg.local_addr.port();
    let fp = bg.fingerprint.clone().unwrap();
    let direct = ServerCfg { address: format!("127.0.0.1:{port}"), tls: true, pin: fp.clone() };
    let ws = ServerCfg { address: format!("ws://127.0.0.1:{port}/"), tls: false, pin: String::new() };
    // wss direto no servidor (certificado fixado) também funciona.
    let wss = client::start(ServerCfg { address: format!("wss://127.0.0.1:{port}"), tls: true, pin: fp }, [8u8; 32], "C".into());
    assert!(wait(&wss.rx, 10, |e| matches!(e, Event::Online { .. })).is_some(), "wss não conectou");
    wss.tx.send(client::Command::Quit).unwrap();

    let host = client::start(direct.clone(), [6u8; 32], "Anfitrião".into());
    let viewer = client::start(ws.clone(), [7u8; 32], "Visualizador".into());
    let Some(Event::Online { id: host_id }) = wait(&host.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("anfitrião offline") };
    let Some(Event::Online { id: viewer_id }) = wait(&viewer.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("visualizador (WebSocket) offline") };
    // O botão "Testar" das configurações: pelo WebSocket, o mesmo servidor devolve o mesmo ID.
    assert_eq!(client::probe(&ws, &[7u8; 32], "Visualizador").unwrap(), viewer_id);
    assert!(client::probe(&ServerCfg { address: "ws://127.0.0.1:9".into(), tls: false, pin: String::new() }, &[7u8; 32], "x").is_err());
    if std::env::var("DISPLAY").is_err() {
        eprintln!("sem DISPLAY: sessão pulada");
        return;
    }
    viewer.tx.send(client::Command::Connect { target: host_id.clone() }).unwrap();
    let Some(Event::SessionReady { session: vs, secret: vsec, .. }) = wait(&viewer.rx, 10, |e| matches!(e, Event::SessionReady { .. })) else { panic!("sem sessão") };
    let Some(Event::Incoming { session: hs, secret: hsec, from_id, .. }) = wait(&host.rx, 5, |e| matches!(e, Event::Incoming { .. })) else { panic!("convite não chegou") };
    assert_eq!((hs, &from_id), (vs, &viewer_id));
    let (htx, hrx) = channel();
    let opt = HostOptions { passwords: vec!["s3nha-ws".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default(), restart: restart_simulado, terminal: terminal_simulado, audio: true, id: host_id.clone(), tech_ca: None };
    let _hh = session::host(direct, hs, hsec, from_id, "Visualizador".into(), opt, htx);
    let remote = Arc::new(Mutex::new(Remote::default()));
    let (vtx, vrx) = channel();
    let vh = session::viewer(ws, vs, vsec, "s3nha-ws".into(), viewer_id.clone(), "Visualizador".into(), dir.join("baixados"), remote.clone(), vtx, host_id.clone(), None);
    assert!(wait(&vrx, 15, |e| matches!(e, ViewerEvent::Connected { .. })).is_some(), "sessão por WebSocket não conectou");
    let end = Instant::now() + Duration::from_secs(15);
    while remote.lock().unwrap().version == 0 {
        assert!(Instant::now() < end, "nenhum quadro recebido pelo WebSocket");
        std::thread::sleep(Duration::from_millis(50));
    }
    vh.cmd.send(session::Cmd::Chat("via túnel".into())).unwrap();
    assert!(wait(&hrx, 5, |e| matches!(e, HostEvent::Chat(t) if t == "via túnel")).is_some());
    let f = dir.join("ws.bin");
    std::fs::write(&f, (0..500_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>()).unwrap();
    vh.cmd.send(session::Cmd::SendFile(f.clone())).unwrap();
    let Some(HostEvent::File(t)) = wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.finished)) else { panic!("arquivo não chegou") };
    assert_eq!(std::fs::read(t.path.unwrap()).unwrap(), std::fs::read(&f).unwrap());
    // Painel do servidor: os dois online e a sessão em andamento.
    let st = bg.stats();
    assert!(st.online.iter().any(|d| d.id == host_id && d.name == "Anfitrião"));
    assert!(st.online.iter().any(|d| d.id == viewer_id));
    assert!(st.active.iter().any(|x| x.host_id == host_id && x.viewer_name == "Visualizador"));
    vh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::Ended(_))).is_some());
    // Ao terminar, vai para o histórico (com nomes e volume).
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let st = bg.stats();
        if let Some(x) = st.recent.iter().find(|x| x.host_id == host_id) {
            assert_eq!(x.host_name, "Anfitrião");
            assert!(x.bytes > 500_000 && x.end >= x.start);
            assert!(st.active.is_empty());
            break;
        }
        assert!(Instant::now() < end, "a sessão não entrou no histórico");
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(bg);
    let _ = std::fs::remove_dir_all(dir);
}

/// Retenção do histórico (LGPD): sessões mais antigas que o prazo são apagadas.
#[test]
fn session_log_retention() {
    let dir = std::env::temp_dir().join(format!("elo-retencao-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let velha = agora - 100 * 86_400;
    let nova = agora - 86_400;
    std::fs::write(dir.join("sessoes.jsonl"), format!("{{\"anfitriao\":\"111111111\",\"visualizador\":\"222222222\",\"inicio\":{velha},\"fim\":{velha},\"bytes\":1}}\n{{\"anfitriao\":\"333333333\",\"visualizador\":\"444444444\",\"inicio\":{nova},\"fim\":{nova},\"bytes\":2}}\n")).unwrap();
    // Na subida, com 90 dias: some só a de 100 dias atrás.
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90, trusted_proxies: Vec::new() }).unwrap();
    let ids: Vec<String> = bg.stats().recent.iter().map(|x| x.host_id.clone()).collect();
    assert_eq!(ids, vec!["333333333".to_string()]);
    // Prazo menor que a idade da que sobrou: o histórico fica vazio.
    bg.set_retention_days(0);
    assert_eq!(bg.stats().recent.len(), 1, "0 = guardar para sempre");
    std::fs::write(dir.join("sessoes.jsonl"), format!("{{\"anfitriao\":\"333333333\",\"visualizador\":\"444444444\",\"inicio\":{nova},\"fim\":{velha},\"bytes\":2}}\n")).unwrap();
    bg.set_retention_days(30);
    assert!(bg.stats().recent.is_empty());
    drop(bg);
    let _ = std::fs::remove_dir_all(dir);
}

/// Wake-on-LAN: o servidor guarda o MAC de quem se apresentou e, quando
/// alguém pede para ligar esse computador, repassa aos Elos da rede dele.
#[test]
fn wake_on_lan_relay() {
    use elo_proto::ctrl::{FromServer, ToServer};
    use elo_proto::frame::{read_frame, write_frame};
    let dir = std::env::temp_dir().join(format!("elo-wol-{}", std::process::id()));
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90, trusted_proxies: Vec::new() }).unwrap();
    let cfg = ServerCfg { address: format!("127.0.0.1:{}", bg.local_addr.port()), tls: true, pin: bg.fingerprint.clone().unwrap() };
    // O computador que vai ser desligado se apresenta com o MAC dele e sai.
    let mut c = elo_core::net::connect(&cfg).unwrap();
    let hello = ToServer::Hello { token: [12u8; 32], name: "Desligado".into(), os: "Linux".into(), version: elo_proto::PROTOCOL_VERSION, mac: "aa:bb:cc:dd:ee:ff".into() };
    write_frame(&mut c, &postcard::to_stdvec(&hello).unwrap()).unwrap();
    let FromServer::Welcome { id: off } = postcard::from_bytes(&read_frame(&mut c).unwrap()).unwrap() else { panic!() };
    drop(c);
    let saved = std::fs::read_to_string(dir.join("dispositivos.json")).unwrap();
    assert!(saved.contains("aa:bb:cc:dd:ee:ff"), "{saved}");
    // Quem pede está na mesma rede (127.0.0.1): conta como repassador.
    let b = client::start(cfg.clone(), [13u8; 32], "Pede".into());
    let Some(Event::Online { id: b_id }) = wait(&b.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("offline") };
    std::thread::sleep(Duration::from_millis(300));
    b.tx.send(client::Command::Wake { target: off.clone() }).unwrap();
    let Some(Event::WakeSent { target, relays }) = wait(&b.rx, 10, |e| matches!(e, Event::WakeSent { .. } | Event::Error(_))) else { panic!("sem resposta ao pedido de ligar") };
    assert_eq!(target, off);
    assert!(relays >= 1, "nenhum Elo da rede repassou");
    // Computador já ligado: erro claro.
    b.tx.send(client::Command::Wake { target: b_id }).unwrap();
    let Some(Event::Error(m)) = wait(&b.rx, 10, |e| matches!(e, Event::Error(_))) else { panic!() };
    assert!(m.contains("já está ligado"), "{m}");
    drop(bg);
    let _ = std::fs::remove_dir_all(dir);
}

/// Quem pediu acesso (sem senha) e desistiu: o pedido some do anfitrião, em
/// vez de ficar esperando resposta na tela.
#[test]
fn cancelled_request_disappears() {
    let port = 24000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-pedido-{port}"));
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let cfg = ServerCfg { address: format!("127.0.0.1:{port}"), tls: false, pin: String::new() };
    let host = client::start(cfg.clone(), [5u8; 32], "Anfitrião".into());
    let viewer = client::start(cfg.clone(), [6u8; 32], "Visualizador".into());
    let Some(Event::Online { id: host_id }) = wait(&host.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("anfitrião offline") };
    let Some(Event::Online { id: viewer_id }) = wait(&viewer.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("visualizador offline") };
    viewer.tx.send(client::Command::Connect { target: host_id.clone() }).unwrap();
    let Some(Event::SessionReady { session: vs, secret: vsec, .. }) = wait(&viewer.rx, 10, |e| matches!(e, Event::SessionReady { .. })) else { panic!("sem sessão") };
    let Some(Event::Incoming { session: hs, secret: hsec, from_id, .. }) = wait(&host.rx, 5, |e| matches!(e, Event::Incoming { .. })) else { panic!("convite não chegou") };
    let (htx, hrx) = channel();
    let opt = HostOptions { passwords: vec!["s3nha-temp".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default(), restart: restart_simulado, terminal: terminal_simulado, audio: true, id: host_id.clone(), tech_ca: None };
    let _hh = session::host(cfg.clone(), hs, hsec, from_id, "Visualizador".into(), opt, htx);
    let (vtx, vrx) = channel();
    let vh = session::viewer(cfg.clone(), vs, vsec, String::new(), viewer_id, "Visualizador".into(), dir.join("baixados"), Arc::new(Mutex::new(Remote::default())), vtx, host_id.clone(), None);
    // Sem senha: vira pedido na tela do anfitrião.
    // (Guarda o pedido, como a tela do app: ninguém respondeu ainda.)
    let Some(HostEvent::Ask { reply: _pending, .. }) = wait(&hrx, 10, |e| matches!(e, HostEvent::Ask { .. })) else { panic!("o pedido não chegou ao anfitrião") };
    assert!(wait(&vrx, 5, |e| matches!(e, ViewerEvent::Waiting { sas } if !sas.is_empty())).is_some(), "o código de verificação tem de aparecer antes do aceite");
    // Quem pediu cancela.
    vh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let ended = wait(&hrx, 8, |e| matches!(e, HostEvent::Ended(_)));
    let Some(HostEvent::Ended(r)) = ended else { panic!("o anfitrião continuou esperando a resposta") };
    assert_eq!(r, session::GAVE_UP);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Empresas: primeiro administrador, empresa, Elo Cliente pelo convite,
/// regras de quem conecta em quem, pedido de ajuda e entrada do técnico.
#[test]
fn companies_techs_and_help_requests() {
    use elo_proto::ctrl::Manage;
    let port = 25000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-empresas-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let cfg = ServerCfg { address: format!("127.0.0.1:{port}"), tls: false, pin: String::new() };
    let setup = std::fs::read_to_string(dir.join("primeiro-acesso.txt")).unwrap();
    let code = setup.split_whitespace().find(|w| w.len() == 9 && w.contains('-')).expect("código no arquivo").to_string();

    // Técnico: sem técnicos ainda, o servidor pede o primeiro acesso.
    let tech = client::start(cfg.clone(), [11u8; 32], "Notebook do técnico".into());
    let Some(Event::Online { id: tech_id }) = wait(&tech.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("técnico offline") };
    assert!(matches!(wait(&tech.rx, 5, |e| matches!(e, Event::Features(_))), Some(Event::Features(f)) if f.contains(&"empresas".to_string())));
    let login = |password: &str, setup: Option<String>| client::Command::Login { user: "Ana".into(), password: password.into(), setup, name: "Ana Souza".into() };
    tech.tx.send(login("s3nha-da-ana", None)).unwrap();
    assert!(wait(&tech.rx, 5, |e| matches!(e, Event::NeedsSetup)).is_some(), "devia pedir o primeiro acesso");
    tech.tx.send(login("s3nha-da-ana", Some(code.clone()))).unwrap();
    let Some(Event::LoggedIn { user, admin, token, .. }) = wait(&tech.rx, 10, |e| matches!(e, Event::LoggedIn { .. })) else { panic!("não entrou") };
    assert_eq!(user, "ana");
    assert!(admin);
    assert!(!dir.join("primeiro-acesso.txt").exists(), "o código de primeiro acesso devia sumir");
    let gestao = std::fs::read_to_string(dir.join("gestao.json")).unwrap();
    assert!(!gestao.contains("s3nha-da-ana") && gestao.contains("$argon2"), "a senha não pode ficar em texto");

    // Empresa nova.
    tech.tx.send(client::Command::Manage(Manage::CreateCompany { name: "Padaria Pão Quente".into() })).unwrap();
    let Some(Event::Overview(o)) = wait(&tech.rx, 5, |e| matches!(e, Event::Overview(o) if !o.companies.is_empty())) else { panic!("empresa não apareceu") };
    let company = o.companies[0].clone();

    // Elo Cliente com o convite.
    let ident = client::Identity { company: Some((company.id.clone(), company.key.clone())), tech_token: None, tech_key: String::new() };
    let cli = client::start_with(cfg.clone(), [12u8; 32], "Caixa da padaria".into(), ident);
    let Some(Event::Online { id: cli_id }) = wait(&cli.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!("cliente offline") };
    assert!(matches!(wait(&cli.rx, 5, |e| matches!(e, Event::Company { .. })), Some(Event::Company { name, .. }) if name == "Padaria Pão Quente"));
    let Some(Event::Overview(o)) = wait(&tech.rx, 5, |e| matches!(e, Event::Overview(o) if o.devices.iter().any(|d| d.company.is_some()))) else { panic!("cliente não apareceu no painel") };
    let d = o.devices.iter().find(|d| d.id == cli_id).unwrap();
    assert!(d.online && d.company.as_deref() == Some(company.id.as_str()));

    // Convite errado é recusado.
    let bad = client::start_with(cfg.clone(), [13u8; 32], "Intruso".into(), client::Identity { company: Some((company.id.clone(), "chave-errada".into())), tech_token: None, tech_key: String::new() });
    assert!(matches!(wait(&bad.rx, 10, |e| matches!(e, Event::Error(_))), Some(Event::Error(m)) if m.contains("convite")));

    // Regras: computador comum não conecta no cliente; o cliente não conecta em ninguém.
    bad.tx.send(client::Command::Connect { target: cli_id.clone() }).unwrap();
    assert!(matches!(wait(&bad.rx, 5, |e| matches!(e, Event::Error(_))), Some(Event::Error(m)) if m.contains("técnicos")));
    cli.tx.send(client::Command::Connect { target: tech_id.clone() }).unwrap();
    assert!(matches!(wait(&cli.rx, 5, |e| matches!(e, Event::Error(_))), Some(Event::Error(m)) if m.contains("só recebe")));

    // Pedido de ajuda → painel; técnico assume → cliente sabe quem vai atender.
    cli.tx.send(client::Command::RequestHelp("a impressora parou".into())).unwrap();
    assert!(wait(&cli.rx, 5, |e| matches!(e, Event::HelpStatus { taken_by: None })).is_some());
    assert!(wait(&tech.rx, 5, |e| matches!(e, Event::Overview(o) if o.requests.iter().any(|r| r.message == "a impressora parou"))).is_some());
    tech.tx.send(client::Command::Manage(Manage::TakeRequest { device: cli_id.clone() })).unwrap();
    assert!(matches!(wait(&cli.rx, 5, |e| matches!(e, Event::HelpStatus { taken_by: Some(_) })), Some(Event::HelpStatus { taken_by: Some(n) }) if n == "Ana Souza"));

    // Técnico conecta: o convite chega ao cliente com o nome do técnico.
    tech.tx.send(client::Command::Connect { target: cli_id.clone() }).unwrap();
    assert!(wait(&tech.rx, 5, |e| matches!(e, Event::SessionReady { .. })).is_some());
    assert!(matches!(wait(&cli.rx, 5, |e| matches!(e, Event::Incoming { .. })), Some(Event::Incoming { from_name, .. }) if from_name == "Ana Souza (técnico)"));

    // Senha errada; e retomar com o token noutro app.
    let other = client::start(cfg.clone(), [14u8; 32], "Outro".into());
    assert!(wait(&other.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
    other.tx.send(client::Command::Login { user: "ana".into(), password: "errada-errada".into(), setup: None, name: String::new() }).unwrap();
    assert!(wait(&other.rx, 10, |e| matches!(e, Event::AuthError(_))).is_some());
    other.tx.send(client::Command::SetIdentity(client::Identity { company: None, tech_token: Some(token.clone()), tech_key: String::new() })).unwrap();
    other.tx.send(client::Command::Quit).unwrap();
    let again = client::start_with(cfg.clone(), [14u8; 32], "Outro".into(), client::Identity { company: None, tech_token: Some(token), tech_key: String::new() });
    assert!(matches!(wait(&again.rx, 10, |e| matches!(e, Event::LoggedIn { .. })), Some(Event::LoggedIn { user, .. }) if user == "ana"));

    // Quem não é técnico não consegue aparecer como "(técnico)".
    let fake = client::start(cfg.clone(), [15u8; 32], "Falso".into());
    assert!(wait(&fake.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
    fake.tx.send(client::Command::Connect { target: tech_id.clone() }).unwrap();
    let Some(Event::Incoming { from_name, .. }) = wait(&tech.rx, 5, |e| matches!(e, Event::Incoming { .. })) else { panic!("convite não chegou") };
    assert!(!from_name.contains('('), "{from_name}");

    // Técnico removido perde o acesso na conexão que já estava aberta.
    tech.tx.send(client::Command::Manage(Manage::CreateTech { user: "beto".into(), name: "Beto".into(), password: "s3nha-do-beto".into(), admin: false })).unwrap();
    assert!(wait(&tech.rx, 5, |e| matches!(e, Event::Overview(o) if o.techs.iter().any(|t| t.user == "beto"))).is_some());
    let beto = client::start(cfg.clone(), [16u8; 32], "Notebook do Beto".into());
    assert!(wait(&beto.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
    beto.tx.send(client::Command::Login { user: "beto".into(), password: "s3nha-do-beto".into(), setup: None, name: String::new() }).unwrap();
    assert!(wait(&beto.rx, 10, |e| matches!(e, Event::LoggedIn { .. })).is_some());
    // Beto não é administrador: não troca a senha de ninguém, nem a própria sem a atual.
    beto.tx.send(client::Command::Manage(Manage::SetPassword { user: "beto".into(), password: "outra-senha".into() })).unwrap();
    assert!(wait(&beto.rx, 5, |e| matches!(e, Event::Error(m) if m.contains("administrador"))).is_some());
    tech.tx.send(client::Command::Manage(Manage::DeleteTech { user: "beto".into() })).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    beto.tx.send(client::Command::Connect { target: cli_id.clone() }).unwrap();
    let refused = wait(&beto.rx, 5, |e| matches!(e, Event::SessionReady { .. } | Event::Error(_) | Event::AuthError(_)));
    assert!(!matches!(refused, Some(Event::SessionReady { .. }) | None), "técnico removido ainda conectou");

    // O histórico guarda qual técnico fez o acesso (quando a sessão abre e fecha).
    for c in [&tech, &cli, &bad, &again, &fake, &beto] {
        let _ = c.tx.send(client::Command::Quit);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Senha errada repetida bloqueia só quem errou: o dono, de outro computador,
/// continua entrando, e errar e desistir no meio também conta.
#[test]
fn wrong_passwords_lock_only_who_tried() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("sem DISPLAY: teste pulado");
        return;
    }
    session::AUDIO_ON_START.store(false, std::sync::atomic::Ordering::Relaxed);
    let port = 30000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-bloqueio-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let cfg = ServerCfg { address: format!("127.0.0.1:{port}"), tls: false, pin: String::new() };
    let host = client::start(cfg.clone(), [21u8; 32], "Anfitrião".into());
    let attacker = client::start(cfg.clone(), [22u8; 32], "Atacante".into());
    let owner = client::start(cfg.clone(), [23u8; 32], "Dono".into());
    let Some(Event::Online { id: host_id }) = wait(&host.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!() };
    let Some(Event::Online { id: attacker_id }) = wait(&attacker.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!() };
    let Some(Event::Online { id: owner_id }) = wait(&owner.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!() };
    // Uma tentativa: devolve se a sessão abriu.
    let attempt = |who: &client::Control, who_id: &str, pw: &str| -> bool {
        who.tx.send(client::Command::Connect { target: host_id.clone() }).unwrap();
        let Some(Event::SessionReady { session: s, secret, .. }) = wait(&who.rx, 10, |e| matches!(e, Event::SessionReady { .. })) else { panic!("sem sessão") };
        let Some(Event::Incoming { from_id, .. }) = wait(&host.rx, 5, |e| matches!(e, Event::Incoming { .. })) else { panic!("convite não chegou") };
        let (htx, _hrx) = channel();
        let opt = HostOptions { passwords: vec!["certa-123".into()], name: "Anfitrião".into(), downloads: dir.join("r"), default_permissions: Default::default(), restart: restart_simulado, terminal: terminal_simulado, audio: false, id: host_id.clone(), tech_ca: None };
        let hh = session::host(cfg.clone(), s, secret, from_id, "x".into(), opt, htx);
        let (vtx, vrx) = channel();
        let vh = session::viewer(cfg.clone(), s, secret, pw.into(), who_id.into(), "x".into(), dir.join("b"), Arc::new(Mutex::new(Remote::default())), vtx, host_id.clone(), None);
        let r = wait(&vrx, 15, |e| matches!(e, ViewerEvent::Connected { .. } | ViewerEvent::Ended(_)));
        vh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        hh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(200));
        matches!(r, Some(ViewerEvent::Connected { .. }))
    };
    for _ in 0..5 {
        assert!(!attempt(&attacker, &attacker_id, "errada"));
    }
    assert!(!attempt(&attacker, &attacker_id, "certa-123"), "quem errou 5 vezes devia estar bloqueado");
    assert!(attempt(&owner, &owner_id, "certa-123"), "o dono, de outro computador, devia entrar");
    for c in [&host, &attacker, &owner] {
        let _ = c.tx.send(client::Command::Quit);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Chave dos técnicos: o administrador cria a chave e aprova um técnico; o
/// computador da empresa (com a chave do convite) aceita o pedido dele como
/// verificado e recusa o de um técnico que ninguém aprovou.
#[test]
fn only_approved_techs_reach_company_computers() {
    use elo_proto::cert;
    use elo_proto::ctrl::Manage;
    if std::env::var("DISPLAY").is_err() {
        eprintln!("sem DISPLAY: teste pulado");
        return;
    }
    session::AUDIO_ON_START.store(false, std::sync::atomic::Ordering::Relaxed);
    let port = 31000 + (std::process::id() % 1000) as u16;
    let dir = std::env::temp_dir().join(format!("elo-e2e-chave-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    let _srv = Server(Command::new(env!("CARGO_BIN_EXE_elo-server")).args(["--listen", &format!("127.0.0.1:{port}"), "--data", dir.to_str().unwrap(), "--no-tls"]).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(500));
    let cfg = ServerCfg { address: format!("127.0.0.1:{port}"), tls: false, pin: String::new() };
    let setup = std::fs::read_to_string(dir.join("primeiro-acesso.txt")).unwrap();
    let code = setup.split_whitespace().find(|w| w.len() == 9 && w.contains('-')).unwrap().to_string();
    let tech = |seed: u8, secret: &[u8; 32]| client::start_with(cfg.clone(), [seed; 32], format!("Técnico {seed}"), client::Identity { company: None, tech_token: None, tech_key: cert::to_hex(&cert::public_of(secret)) });

    // Administrador: entra, cria a chave dos técnicos e os técnicos.
    let (admin_key, ca) = (cert::new_secret(), cert::new_secret());
    let admin = tech(41, &admin_key);
    assert!(wait(&admin.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
    admin.tx.send(client::Command::Login { user: "ana".into(), password: "s3nha-da-ana".into(), setup: Some(code), name: "Ana".into() }).unwrap();
    assert!(wait(&admin.rx, 10, |e| matches!(e, Event::LoggedIn { .. })).is_some());
    admin.tx.send(client::Command::Manage(Manage::SetCa { public: cert::to_hex(&cert::public_of(&ca)) })).unwrap();
    for u in ["beto", "caio"] {
        admin.tx.send(client::Command::Manage(Manage::CreateTech { user: u.into(), name: u.to_uppercase(), password: format!("s3nha-do-{u}"), admin: false })).unwrap();
    }
    admin.tx.send(client::Command::Manage(Manage::CreateCompany { name: "Clínica".into() })).unwrap();
    let Some(Event::Overview(o)) = wait(&admin.rx, 10, |e| matches!(e, Event::Overview(o) if !o.ca.is_empty() && o.techs.len() == 3 && !o.companies.is_empty())) else { panic!("painel sem chave, técnicos ou empresa") };
    let company = o.companies[0].clone();

    // Beto e Caio entram, cada um com a chave do próprio app.
    let (beto_key, caio_key) = (cert::new_secret(), cert::new_secret());
    let beto = tech(42, &beto_key);
    let caio = tech(43, &caio_key);
    for (c, u) in [(&beto, "beto"), (&caio, "caio")] {
        assert!(wait(&c.rx, 10, |e| matches!(e, Event::Online { .. })).is_some());
        c.tx.send(client::Command::Login { user: u.into(), password: format!("s3nha-do-{u}"), setup: None, name: String::new() }).unwrap();
        assert!(wait(&c.rx, 10, |e| matches!(e, Event::LoggedIn { .. })).is_some());
    }
    // O administrador aprova só o Beto (a chave que o painel mostra).
    let beto_pub = cert::to_hex(&cert::public_of(&beto_key));
    assert!(wait(&admin.rx, 10, |e| matches!(e, Event::Overview(o) if o.techs.iter().any(|t| t.user == "beto" && t.keys.iter().any(|k| k.key == beto_pub)))).is_some(), "a chave do Beto não chegou ao painel");
    let signed = cert::issue(&ca, cert::TechCert { user: "beto".into(), name: "Beto".into(), key: cert::public_of(&beto_key), expires: u64::MAX });
    admin.tx.send(client::Command::Manage(Manage::Certify { user: "beto".into(), cert: signed.to_text() })).unwrap();
    // Certificado de outra chave dos técnicos é recusado pelo servidor.
    let forged = cert::issue(&cert::new_secret(), cert::TechCert { user: "caio".into(), name: "Caio".into(), key: cert::public_of(&caio_key), expires: u64::MAX });
    admin.tx.send(client::Command::Manage(Manage::Certify { user: "caio".into(), cert: forged.to_text() })).unwrap();
    assert!(wait(&admin.rx, 5, |e| matches!(e, Event::Error(m) if m.contains("não confere"))).is_some());
    let Some(Event::Overview(bo)) = wait(&beto.rx, 10, |e| matches!(e, Event::Overview(o) if o.me.keys.iter().any(|k| !k.cert.is_empty()))) else { panic!("o Beto não recebeu o certificado") };
    let beto_cert = cert::SignedCert::from_text(&bo.me.keys.iter().find(|k| k.key == beto_pub).unwrap().cert).unwrap();

    // Computador da empresa: convite com a chave dos técnicos.
    let cli = client::start_with(cfg.clone(), [44u8; 32], "Recepção".into(), client::Identity { company: Some((company.id.clone(), company.key.clone())), tech_token: None, tech_key: String::new() });
    let Some(Event::Online { id: cli_id }) = wait(&cli.rx, 10, |e| matches!(e, Event::Online { .. })) else { panic!() };
    let Some(Event::Company { ca: announced, .. }) = wait(&cli.rx, 10, |e| matches!(e, Event::Company { .. })) else { panic!("sem empresa") };
    assert_eq!(announced, cert::to_hex(&cert::public_of(&ca)));
    let ca_pub = cert::public_of(&ca);

    // Um pedido de cada técnico: devolve (o anfitrião perguntou, verificado, fim do visualizador).
    let attempt = |who: &client::Control, my_id: &str, credential: Option<elo_proto::secure::Credential>| {
        who.tx.send(client::Command::Connect { target: cli_id.clone() }).unwrap();
        let Some(Event::SessionReady { session: s, secret, .. }) = wait(&who.rx, 10, |e| matches!(e, Event::SessionReady { .. })) else { panic!("sem sessão") };
        let Some(Event::Incoming { from_id, from_name, .. }) = wait(&cli.rx, 5, |e| matches!(e, Event::Incoming { .. })) else { panic!("convite não chegou") };
        let (htx, hrx) = channel();
        let opt = HostOptions { passwords: Vec::new(), name: "Recepção".into(), downloads: dir.join("r"), default_permissions: Default::default(), restart: restart_simulado, terminal: terminal_simulado, audio: false, id: cli_id.clone(), tech_ca: Some(ca_pub) };
        let hh = session::host(cfg.clone(), s, secret, from_id, from_name, opt, htx);
        let (vtx, vrx) = channel();
        let vh = session::viewer(cfg.clone(), s, secret, String::new(), my_id.into(), "x".into(), dir.join("b"), Arc::new(Mutex::new(Remote::default())), vtx, cli_id.clone(), credential);
        let ask = wait(&hrx, 8, |e| matches!(e, HostEvent::Ask { .. }));
        let end = if ask.is_some() { None } else { wait(&vrx, 8, |e| matches!(e, ViewerEvent::Ended(_))) };
        vh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        hh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(300));
        match (ask, end) {
            (Some(HostEvent::Ask { verified, from_name, .. }), _) => (true, verified, from_name),
            (_, Some(ViewerEvent::Ended(r))) => (false, None, r),
            _ => panic!("nem pedido nem fim"),
        }
    };
    // IDs dos técnicos (o mesmo token devolve o mesmo ID).
    let beto_ctl_id = client::probe(&cfg, &[42u8; 32], "Técnico 42").unwrap();
    let caio_ctl_id = client::probe(&cfg, &[43u8; 32], "Técnico 43").unwrap();
    let (asked, verified, name) = attempt(&beto, &beto_ctl_id, Some(elo_proto::secure::Credential { secret: beto_key, cert: beto_cert.to_bytes() }));
    assert!(asked, "o pedido do Beto devia aparecer");
    assert_eq!(verified.as_deref(), Some("Beto"));
    assert_eq!(name, "Beto (técnico)");
    let (asked, _, reason) = attempt(&caio, &caio_ctl_id, None);
    assert!(!asked, "o pedido do Caio (não aprovado) não devia aparecer");
    assert!(reason.contains("aprovados"), "{reason}");
    // Certificado do Beto com a chave do Caio: a assinatura da sessão não bate.
    let (asked, _, _) = attempt(&caio, &caio_ctl_id, Some(elo_proto::secure::Credential { secret: caio_key, cert: beto_cert.to_bytes() }));
    assert!(!asked, "certificado alheio não devia passar");
    for c in [&admin, &beto, &caio, &cli] {
        let _ = c.tx.send(client::Command::Quit);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
