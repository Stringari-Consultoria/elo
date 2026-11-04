//! Servidor do Elo.
//!
//! - Registra dispositivos e devolve um ID de 9 dígitos estável (guarda só o
//!   hash do segredo do dispositivo).
//! - Encaminha convites de conexão ao anfitrião.
//! - Faz o relay das sessões: depois do pareamento, só repassa bytes já
//!   cifrados ponta a ponta pelos apps (não tem a chave).
//! - Registra metadados das sessões (IDs, início, fim, volume) para operação
//!   e cobrança futura — nunca o conteúdo.
//!
//! Uso: `elo-server --listen 0.0.0.0:21116 --data /var/lib/elo [--cert c.pem --key k.pem | --self-signed | --no-tls]`

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use elo_proto::ctrl::{ErrorCode, FromServer, ToServer};
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, oneshot};

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type BoxStream = Box<dyn Stream>;

async fn read_frame(s: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 * 1024 {
        return Err(std::io::Error::other("mensagem de controle grande demais"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_msg(s: &mut (impl AsyncWrite + Unpin), m: &FromServer) -> std::io::Result<()> {
    let data = postcard::to_stdvec(m).map_err(std::io::Error::other)?;
    s.write_all(&(data.len() as u32).to_le_bytes()).await?;
    s.write_all(&data).await?;
    s.flush().await
}

struct Device {
    tx: mpsc::UnboundedSender<FromServer>,
    name: String,
    os: String,
}

struct Pending {
    secret: [u8; 16],
    host_id: String,
    viewer_id: String,
    created: Instant,
    /// Primeiro lado que chegou esperando o outro.
    waiting: Option<(bool, oneshot::Sender<BoxStream>)>,
}

#[derive(Default)]
struct Limits {
    hits: HashMap<String, Vec<Instant>>,
}

impl Limits {
    /// No máximo `n` eventos por minuto para a chave.
    fn allow(&mut self, key: &str, n: usize) -> bool {
        let now = Instant::now();
        let v = self.hits.entry(key.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if v.len() >= n {
            return false;
        }
        v.push(now);
        true
    }
}

struct State {
    data: PathBuf,
    /// hash(token) → ID
    ids: Mutex<HashMap<String, String>>,
    /// Conexões ativas por ID (a mais recente recebe os convites).
    online: Mutex<HashMap<String, Vec<Device>>>,
    pending: Mutex<HashMap<u64, Pending>>,
    limits: Mutex<Limits>,
}

impl State {
    async fn save_ids(&self) {
        let ids = self.ids.lock().await;
        if let Ok(j) = serde_json::to_vec_pretty(&*ids) {
            let tmp = self.data.join("ids.json.tmp");
            if tokio::fs::write(&tmp, j).await.is_ok() {
                let _ = tokio::fs::rename(&tmp, self.data.join("ids.json")).await;
            }
        }
    }

    async fn log_session(&self, line: serde_json::Value) {
        use tokio::io::AsyncWriteExt as _;
        if let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open(self.data.join("sessoes.jsonl")).await {
            let _ = f.write_all(format!("{line}\n").as_bytes()).await;
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

async fn control(state: Arc<State>, s: BoxStream, peer: SocketAddr, token: [u8; 32], name: String, os: String, version: u32) {
    let (mut rd, mut wr) = tokio::io::split(s);
    if version != elo_proto::PROTOCOL_VERSION {
        let _ = write_msg(&mut wr, &FromServer::Error { code: ErrorCode::Version, message: "atualize o Elo para conectar".into() }).await;
        return;
    }
    if !state.limits.lock().await.allow(&format!("hello:{}", peer.ip()), 30) {
        let _ = write_msg(&mut wr, &FromServer::Error { code: ErrorCode::RateLimited, message: "muitas tentativas, aguarde um minuto".into() }).await;
        return;
    }
    let key = format!("{:x}", Sha256::digest(token));
    let id = {
        let mut ids = state.ids.lock().await;
        if let Some(id) = ids.get(&key) {
            id.clone()
        } else {
            let used: std::collections::HashSet<String> = ids.values().cloned().collect();
            let mut rng = rand::thread_rng();
            let id = loop {
                let n: u32 = rng.gen_range(100_000_000..1_000_000_000);
                let s = n.to_string();
                if !used.contains(&s) {
                    break s;
                }
            };
            ids.insert(key, id.clone());
            id
        }
    };
    state.save_ids().await;
    let (tx, mut rx) = mpsc::unbounded_channel::<FromServer>();
    {
        let mut on = state.online.lock().await;
        on.entry(id.clone()).or_default().push(Device { tx: tx.clone(), name: name.clone(), os: os.clone() });
    }
    eprintln!("[+] {id} ({name}, {os}) de {peer}");
    if write_msg(&mut wr, &FromServer::Welcome { id: id.clone() }).await.is_err() {
        return;
    }
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if write_msg(&mut wr, &m).await.is_err() {
                break;
            }
        }
    });
    loop {
        let frame = match tokio::time::timeout(Duration::from_secs(90), read_frame(&mut rd)).await {
            Ok(Ok(f)) => f,
            _ => break,
        };
        let Ok(m) = postcard::from_bytes::<ToServer>(&frame) else { break };
        match m {
            ToServer::Ping => {
                let _ = tx.send(FromServer::Pong);
            }
            ToServer::Connect { target, from_name } => {
                let target = elo_proto::ctrl::clean_id(&target);
                let ok_ip = state.limits.lock().await.allow(&format!("connect:{}", peer.ip()), 20);
                let ok_target = state.limits.lock().await.allow(&format!("target:{target}"), 30);
                if !ok_ip || !ok_target {
                    let _ = tx.send(FromServer::Error { code: ErrorCode::RateLimited, message: "muitas tentativas de conexão; aguarde um minuto".into() });
                    continue;
                }
                if target == id {
                    let _ = tx.send(FromServer::Error { code: ErrorCode::BadRequest, message: "esse é o ID deste computador".into() });
                    continue;
                }
                let on = state.online.lock().await;
                let Some(host) = on.get(&target).and_then(|v| v.last()) else {
                    let known = state.ids.lock().await.values().any(|v| *v == target);
                    let _ = tx.send(FromServer::Error { code: if known { ErrorCode::Offline } else { ErrorCode::NotFound }, message: if known { "o computador está desligado ou sem o Elo aberto".into() } else { "ID não encontrado".into() } });
                    continue;
                };
                let session: u64 = rand::thread_rng().r#gen();
                let secret: [u8; 16] = rand::thread_rng().r#gen();
                let _ = host.tx.send(FromServer::Incoming { session, secret, from_id: id.clone(), from_name: from_name.clone() });
                let _ = tx.send(FromServer::SessionReady { session, secret, target_name: host.name.clone(), target_os: host.os.clone() });
                drop(on);
                state.pending.lock().await.insert(session, Pending { secret, host_id: target, viewer_id: id.clone(), created: Instant::now(), waiting: None });
            }
            ToServer::Decline { session } => {
                if let Some(p) = state.pending.lock().await.remove(&session)
                    && let Some(v) = state.online.lock().await.get(&p.viewer_id).and_then(|v| v.last())
                {
                    let _ = v.tx.send(FromServer::Declined { session });
                }
            }
            _ => {}
        }
    }
    writer.abort();
    let mut on = state.online.lock().await;
    if let Some(v) = on.get_mut(&id) {
        v.retain(|d| !d.tx.same_channel(&tx));
        if v.is_empty() {
            on.remove(&id);
        }
    }
    eprintln!("[-] {id}");
}

/// Pareamento: o primeiro a chegar espera; o segundo entrega seu stream e o
/// primeiro faz o relay entre os dois.
async fn join(state: Arc<State>, mut s: BoxStream, session: u64, secret: [u8; 16], host: bool) {
    let first = {
        let mut p = state.pending.lock().await;
        let Some(pd) = p.get_mut(&session) else {
            let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::Expired, message: "sessão expirada".into() }).await;
            return;
        };
        if pd.secret != secret || pd.created.elapsed() > Duration::from_secs(120) {
            let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::Expired, message: "sessão expirada".into() }).await;
            return;
        }
        match pd.waiting.take() {
            Some((was_host, tx)) if was_host != host => {
                let ids = (pd.host_id.clone(), pd.viewer_id.clone());
                p.remove(&session);
                Some((tx, ids))
            }
            _ => None,
        }
    };
    match first {
        // Segundo a chegar: entrega o próprio stream a quem espera (que faz o relay).
        Some((tx, _)) => {
            let _ = tx.send(s);
        }
        // Primeiro a chegar: espera o outro e faz o relay.
        None => {
            let (tx, rx) = oneshot::channel();
            let ids = {
                let mut p = state.pending.lock().await;
                let Some(pd) = p.get_mut(&session) else { return };
                pd.waiting = Some((host, tx));
                (pd.host_id.clone(), pd.viewer_id.clone())
            };
            let Ok(Ok(mut other)) = tokio::time::timeout(Duration::from_secs(120), rx).await else {
                let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::Expired, message: "o outro lado não entrou".into() }).await;
                state.pending.lock().await.remove(&session);
                return;
            };
            if write_msg(&mut s, &FromServer::Paired).await.is_err() || write_msg(&mut other, &FromServer::Paired).await.is_err() {
                return;
            }
            let start = now_secs();
            eprintln!("[=] sessão {session:x}: {} ← {}", ids.0, ids.1);
            let r = tokio::io::copy_bidirectional(&mut s, &mut other).await;
            let (a, b) = r.unwrap_or((0, 0));
            eprintln!("[x] sessão {session:x}: {} bytes", a + b);
            state.log_session(serde_json::json!({"sessao": format!("{session:x}"), "anfitriao": ids.0, "visualizador": ids.1, "inicio": start, "fim": now_secs(), "bytes": a + b})).await;
        }
    }
}

fn tls_config(args: &Args) -> Option<Arc<rustls::ServerConfig>> {
    if args.no_tls {
        return None;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = if let (Some(c), Some(k)) = (&args.cert, &args.key) {
        let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(std::fs::File::open(c).expect("certificado"))).filter_map(|x| x.ok()).collect();
        let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(std::fs::File::open(k).expect("chave"))).ok().flatten().expect("chave privada");
        (certs, key)
    } else {
        // Autoassinado persistente (os apps fixam a impressão digital).
        let cp = args.data.join("cert.pem");
        let kp = args.data.join("key.pem");
        if !cp.exists() || !kp.exists() {
            let ck = rcgen::generate_simple_self_signed(vec!["elo".into(), "localhost".into()]).expect("certificado");
            std::fs::write(&cp, ck.cert.pem()).expect("gravar certificado");
            std::fs::write(&kp, ck.key_pair.serialize_pem()).expect("gravar chave");
        }
        let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(std::fs::File::open(&cp).expect("certificado"))).filter_map(|x| x.ok()).collect();
        let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(std::fs::File::open(&kp).expect("chave"))).ok().flatten().expect("chave");
        let fp = Sha256::digest(certs[0].as_ref());
        eprintln!("Certificado autoassinado. Impressão digital (SHA-256) para os apps: {}", fp.iter().map(|b| format!("{b:02x}")).collect::<String>());
        (certs, key)
    };
    let cfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key).expect("TLS");
    Some(Arc::new(cfg))
}

struct Args {
    listen: String,
    data: PathBuf,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    no_tls: bool,
}

fn args() -> Args {
    let mut a = Args { listen: format!("0.0.0.0:{}", elo_proto::DEFAULT_PORT), data: PathBuf::from("elo-dados"), cert: None, key: None, no_tls: false };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--listen" => a.listen = it.next().unwrap_or(a.listen),
            "--data" => a.data = it.next().map(PathBuf::from).unwrap_or(a.data),
            "--cert" => a.cert = it.next().map(PathBuf::from),
            "--key" => a.key = it.next().map(PathBuf::from),
            "--no-tls" => a.no_tls = true,
            "--self-signed" => {}
            "-h" | "--help" => {
                println!("elo-server --listen 0.0.0.0:21116 --data DIR [--cert C.pem --key K.pem | --self-signed | --no-tls]");
                std::process::exit(0);
            }
            other => eprintln!("opção desconhecida: {other}"),
        }
    }
    a
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = args();
    std::fs::create_dir_all(&args.data)?;
    let ids: HashMap<String, String> = std::fs::read(args.data.join("ids.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let state = Arc::new(State { data: args.data.clone(), ids: Mutex::new(ids), online: Mutex::new(HashMap::new()), pending: Mutex::new(HashMap::new()), limits: Mutex::new(Limits::default()) });
    let tls = tls_config(&args).map(tokio_rustls::TlsAcceptor::from);
    let listener = TcpListener::bind(&args.listen).await?;
    eprintln!("Elo servidor em {} ({})", args.listen, if tls.is_some() { "TLS" } else { "sem TLS — só para testes" });
    // Limpeza de convites expirados.
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            st.pending.lock().await.retain(|_, p| p.created.elapsed() < Duration::from_secs(150));
            st.limits.lock().await.hits.retain(|_, v| v.last().is_some_and(|t| t.elapsed() < Duration::from_secs(60)));
        }
    });
    loop {
        let (sock, peer) = listener.accept().await?;
        let _ = sock.set_nodelay(true);
        let state = state.clone();
        let tls = tls.clone();
        tokio::spawn(async move {
            let s: BoxStream = match tls {
                Some(acc) => match tokio::time::timeout(Duration::from_secs(10), acc.accept(sock)).await {
                    Ok(Ok(t)) => Box::new(t),
                    _ => return,
                },
                None => Box::new(sock),
            };
            handle(state, s, peer).await;
        });
    }
}

async fn handle(state: Arc<State>, mut s: BoxStream, peer: SocketAddr) {
    let Ok(Ok(first)) = tokio::time::timeout(Duration::from_secs(15), read_frame(&mut s)).await else { return };
    let Ok(msg) = postcard::from_bytes::<ToServer>(&first) else { return };
    match msg {
        ToServer::Hello { token, name, os, version } => control(state, s, peer, token, name, os, version).await,
        ToServer::Join { session, secret, host } => join(state, s, session, secret, host).await,
        _ => {
            let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::BadRequest, message: "comece com Hello ou Join".into() }).await;
        }
    }
}
