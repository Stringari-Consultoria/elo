//! Ponta a ponta: servidor real (sem TLS), anfitrião e visualizador pelo relay.
//! A captura usa o X11 de `DISPLAY`; sem tela, o teste é pulado.

use std::process::{Child, Command};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elo_core::client::{self, Event};
use elo_core::net::ServerCfg;
use elo_core::session::{self, HostEvent, HostOptions, Remote, ViewerEvent};

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
        let opt = HostOptions { passwords: vec!["s3nha-temp".into()], name: "Anfitrião".into(), downloads: dir.join("recebidos"), default_permissions: Default::default() };
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
        vh.cmd.send(session::Cmd::SendFile(f)).unwrap();
        let Some(HostEvent::File(t)) = wait(&hrx, 10, |e| matches!(e, HostEvent::File(t) if t.finished)) else { panic!("arquivo não chegou") };
        assert_eq!(std::fs::metadata(t.path.unwrap()).unwrap().len(), 500_000);
        vh.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(wait(&hrx, 10, |e| matches!(e, HostEvent::Ended(_))).is_some());
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
