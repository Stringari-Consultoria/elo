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

#[test]
fn session_through_relay() {
    if std::env::var("DISPLAY").is_err() {
        eprintln!("sem DISPLAY: teste pulado");
        return;
    }
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
        let opt = HostOptions { passwords: vec!["s3nha-temp".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default(), restart: restart_simulado };
        let hh = session::host(cfg.clone(), hs, hsec, from.clone(), "Visualizador".into(), opt, htx);
        let remote = Arc::new(Mutex::new(Remote::default()));
        let (vtx, vrx) = channel();
        let vh = session::viewer(cfg.clone(), vs, vsec, pw.into(), viewer_id.clone(), "Visualizador".into(), dir.join("baixados"), remote.clone(), vtx);
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
        vh.cmd.send(session::Cmd::Download(shared.join("remoto.txt").to_string_lossy().into_owned())).unwrap();
        let Some(ViewerEvent::File(t)) = wait(&vrx, 10, |e| matches!(e, ViewerEvent::File(t) if t.incoming && t.finished)) else { panic!("download não chegou") };
        assert_eq!(std::fs::read(t.path.unwrap()).unwrap(), vec![b'r'; 1000]);
        vh.cmd.send(session::Cmd::Upload(f.clone(), shared.join("sub").to_string_lossy().into_owned())).unwrap();
        let Some(HostEvent::File(t)) = wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.incoming && t.finished)) else { panic!("upload não chegou") };
        let up = t.path.unwrap();
        assert!(up.starts_with(shared.join("sub")), "{}", up.display());
        assert_eq!(std::fs::metadata(&up).unwrap().len(), 500_000);
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
    let cfg = |listen: String| elo_server::Config { listen, data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90 };
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
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90 }).unwrap();
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
    let opt = HostOptions { passwords: vec!["s3nha-ws".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default(), restart: restart_simulado };
    let _hh = session::host(direct, hs, hsec, from_id, "Visualizador".into(), opt, htx);
    let remote = Arc::new(Mutex::new(Remote::default()));
    let (vtx, vrx) = channel();
    let vh = session::viewer(ws, vs, vsec, "s3nha-ws".into(), viewer_id.clone(), "Visualizador".into(), dir.join("baixados"), remote.clone(), vtx);
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
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90 }).unwrap();
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
    let bg = elo_server::Background::start(elo_server::Config { listen: "127.0.0.1:0".into(), data: dir.clone(), tls: elo_server::Tls::SelfSigned, retention_days: 90 }).unwrap();
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
