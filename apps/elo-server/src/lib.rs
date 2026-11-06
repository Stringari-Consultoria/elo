//! Servidor do Elo (biblioteca).
//!
//! - Registra dispositivos e devolve um ID de 9 dígitos estável (guarda só o
//!   hash do segredo do dispositivo).
//! - Encaminha convites de conexão ao anfitrião.
//! - Faz o relay das sessões: depois do pareamento, só repassa bytes já
//!   cifrados ponta a ponta pelos apps (não tem a chave).
//! - Registra metadados das sessões (IDs, início, fim, volume) para operação
//!   e cobrança futura — nunca o conteúdo.
//!
//! Usado pelo binário `elo-server` e embutido no app (servidor local, que sobe
//! junto com o Elo — ver [`Background`]).
//!
//! Na mesma porta aceita o protocolo direto (TCP + TLS) e WebSocket (para ficar
//! atrás de proxies HTTP como o Cloudflare Tunnel). Sem TLS, só aceita
//! conexões vindas do próprio computador (o `cloudflared`, por exemplo).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use elo_proto::ctrl::{ErrorCode, FromServer, ToServer};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
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
    ip: String,
    since: u64,
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Known {
    name: String,
    os: String,
    mac: String,
    ip: String,
}

struct State {
    data: PathBuf,
    /// hash(token) → ID
    ids: Mutex<HashMap<String, String>>,
    /// Conexões ativas por ID (a mais recente recebe os convites).
    online: Mutex<HashMap<String, Vec<Device>>>,
    pending: Mutex<HashMap<u64, Pending>>,
    limits: Mutex<Limits>,
    /// Sessões no relay agora: id → (anfitrião, visualizador, início).
    active: Mutex<HashMap<u64, (String, String, u64)>>,
    /// Último nome, sistema, MAC e IP de cada ID (`dispositivos.json`): painel
    /// e Wake-on-LAN.
    known: Mutex<HashMap<String, Known>>,
    /// Dias que o histórico de sessões é guardado (0 = para sempre).
    retention_days: std::sync::atomic::AtomicU32,
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

    async fn save_known(&self) {
        let k = self.known.lock().await;
        if let Ok(j) = serde_json::to_vec_pretty(&*k) {
            let tmp = self.data.join("dispositivos.json.tmp");
            if tokio::fs::write(&tmp, j).await.is_ok() {
                let _ = tokio::fs::rename(&tmp, self.data.join("dispositivos.json")).await;
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

/// Apaga do histórico as sessões terminadas há mais de `days` dias.
fn prune_log(data: &Path, days: u32) {
    if days == 0 {
        return;
    }
    let p = data.join("sessoes.jsonl");
    let Ok(text) = std::fs::read_to_string(&p) else { return };
    let limit = now_secs().saturating_sub(days as u64 * 86_400);
    let keep: Vec<&str> = text.lines().filter(|l| serde_json::from_str::<serde_json::Value>(l).ok().and_then(|v| v["fim"].as_u64()).is_none_or(|fim| fim >= limit)).collect();
    if keep.len() == text.lines().count() {
        return;
    }
    let tmp = data.join("sessoes.jsonl.tmp");
    let body = if keep.is_empty() { String::new() } else { format!("{}\n", keep.join("\n")) };
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

async fn control(state: Arc<State>, s: BoxStream, peer: SocketAddr, token: [u8; 32], name: String, os: String, version: u32, mac: String) {
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
        on.entry(id.clone()).or_default().push(Device { tx: tx.clone(), name: name.clone(), os: os.clone(), ip: peer.ip().to_string(), since: now_secs() });
        let k = Known { name: name.clone(), os: os.clone(), mac: if elo_proto::ctrl::magic_packet(&mac).is_some() { mac.clone() } else { String::new() }, ip: peer.ip().to_string() };
        let changed = {
            let mut known = state.known.lock().await;
            let old = known.get(&id).map(|o| (o.name.clone(), o.os.clone(), o.mac.clone(), o.ip.clone()));
            let new = (k.name.clone(), k.os.clone(), k.mac.clone(), k.ip.clone());
            // Sem MAC agora (ex.: versão antiga) não apaga o que já se sabia.
            let mac = if k.mac.is_empty() { old.as_ref().map(|o| o.2.clone()).unwrap_or_default() } else { k.mac.clone() };
            known.insert(id.clone(), Known { mac, ..k });
            old != Some(new)
        };
        if changed {
            state.save_known().await;
        }
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
            ToServer::Wake { target } => {
                let target = elo_proto::ctrl::clean_id(&target);
                if !state.limits.lock().await.allow(&format!("wake:{}", peer.ip()), 10) {
                    let _ = tx.send(FromServer::Error { code: ErrorCode::RateLimited, message: "muitos pedidos para ligar computadores; aguarde um minuto".into() });
                    continue;
                }
                if state.online.lock().await.contains_key(&target) {
                    let _ = tx.send(FromServer::Error { code: ErrorCode::BadRequest, message: "esse computador já está ligado e com o Elo aberto".into() });
                    continue;
                }
                let Some(k) = state.known.lock().await.get(&target).cloned().filter(|k| !k.mac.is_empty()) else {
                    let _ = tx.send(FromServer::Error { code: ErrorCode::NotFound, message: "o servidor ainda não sabe o endereço da placa de rede desse computador (abra nele uma vez a versão nova do Elo)".into() });
                    continue;
                };
                // O próprio servidor manda na rede dele (servidor embutido no escritório)...
                let mac = k.mac.clone();
                let _ = tokio::task::spawn_blocking(move || elo_proto::ctrl::wake_on_lan(&mac)).await;
                // ...e os Elos online na mesma rede do alvo (mesmo IP público) também.
                let mut relays = 0u32;
                for (oid, devs) in state.online.lock().await.iter() {
                    if let Some(d) = devs.last()
                        && d.ip == k.ip
                        && *oid != target
                    {
                        let _ = d.tx.send(FromServer::WakeRequest { mac: k.mac.clone() });
                        relays += 1;
                    }
                }
                eprintln!("[~] {id} pediu para ligar {target} ({relays} na rede dele)");
                let _ = tx.send(FromServer::WakeSent { target, relays });
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

/// Repassa bytes nos dois sentidos até um dos lados fechar ou falhar.
/// Devolve o volume de cada sentido (contado mesmo quando termina com erro,
/// que é o normal ao fechar uma sessão).
///
/// Quando um lado fecha, o outro ganha até 2 s para ler o que já chegou (ex.:
/// o `Bye` de quem fechou). Se a escrita falhar, a leitura continua e descarta,
/// para quem ainda está mandando não levar "broken pipe" antes de ler o aviso.
async fn relay(a: BoxStream, b: BoxStream) -> (u64, u64) {
    async fn pipe(mut r: impl AsyncRead + Unpin, mut w: impl AsyncWrite + Unpin, n: &mut u64) {
        let mut buf = vec![0u8; 32 * 1024];
        let mut open = true;
        loop {
            let k = match r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(k) => k,
            };
            if open {
                if w.write_all(&buf[..k]).await.is_err() || w.flush().await.is_err() {
                    open = false;
                    continue;
                }
                *n += k as u64;
            }
        }
        if open {
            let _ = w.shutdown().await;
        }
    }
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    let (mut up, mut down) = (0u64, 0u64);
    {
        let fa = pipe(ar, bw, &mut up);
        let fb = pipe(br, aw, &mut down);
        tokio::pin!(fa, fb);
        let grace = Duration::from_secs(2);
        tokio::select! {
            _ = &mut fa => { let _ = tokio::time::timeout(grace, &mut fb).await; }
            _ = &mut fb => { let _ = tokio::time::timeout(grace, &mut fa).await; }
        }
    }
    (up, down)
}

/// Pareamento: o primeiro a chegar espera; o segundo entrega seu stream e o
/// primeiro faz o relay entre os dois.
async fn join(state: Arc<State>, mut s: BoxStream, session: u64, secret: [u8; 16], host: bool) {
    enum Step {
        Expired,
        /// Já há alguém do mesmo lado esperando (pedido repetido).
        Duplicate,
        /// Segundo a chegar: entrega o stream a quem espera.
        Second(oneshot::Sender<BoxStream>),
        /// Primeiro a chegar: espera o outro e faz o relay.
        First(oneshot::Receiver<BoxStream>, (String, String)),
    }
    // Verificar e registrar a espera na mesma trava: se os dois lados chegam
    // juntos, um deles é sempre o segundo (antes, os dois podiam se achar o
    // primeiro, e um sobrescrevia a espera do outro).
    let step = {
        let mut p = state.pending.lock().await;
        match p.get_mut(&session) {
            Some(pd) if pd.secret == secret && pd.created.elapsed() <= Duration::from_secs(120) => match pd.waiting.take() {
                Some((was_host, tx)) if was_host != host => {
                    p.remove(&session);
                    Step::Second(tx)
                }
                Some(w) => {
                    pd.waiting = Some(w);
                    Step::Duplicate
                }
                None => {
                    let (tx, rx) = oneshot::channel();
                    pd.waiting = Some((host, tx));
                    Step::First(rx, (pd.host_id.clone(), pd.viewer_id.clone()))
                }
            },
            _ => Step::Expired,
        }
    };
    match step {
        Step::Expired | Step::Duplicate => {
            let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::Expired, message: "sessão expirada".into() }).await;
        }
        Step::Second(tx) => {
            let _ = tx.send(s);
        }
        Step::First(rx, ids) => {
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
            state.active.lock().await.insert(session, (ids.0.clone(), ids.1.clone(), start));
            let (a, b) = relay(s, other).await;
            state.active.lock().await.remove(&session);
            eprintln!("[x] sessão {session:x}: {} bytes", a + b);
            state.log_session(serde_json::json!({"sessao": format!("{session:x}"), "anfitriao": ids.0, "visualizador": ids.1, "inicio": start, "fim": now_secs(), "bytes": a + b})).await;
        }
    }
}

/// Decide entre WebSocket ("GET " no começo) e o protocolo direto (que começa
/// com o tamanho do quadro: "GET " daria um quadro maior que o limite).
async fn handle(state: Arc<State>, s: BoxStream, peer: SocketAddr) {
    use tokio::io::AsyncBufReadExt;
    let mut s = tokio::io::BufReader::with_capacity(16 * 1024, s);
    let is_ws = match tokio::time::timeout(Duration::from_secs(15), s.fill_buf()).await {
        Ok(Ok(b)) if !b.is_empty() => b.starts_with(b"GET "),
        _ => return,
    };
    if is_ws {
        websocket(state, s, peer).await;
    } else {
        dispatch(state, Box::new(s), peer).await;
    }
}

/// Proxy em que se confia para informar o IP real do cliente: o próprio
/// computador ou uma rede privada (ex.: cloudflared num contêiner Docker).
fn trusted_proxy(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback() || m.is_private()),
    }
}

/// IP real do cliente atrás do proxy (só confiável quando o proxy é local).
fn forwarded_ip(req: &tokio_tungstenite::tungstenite::handshake::server::Request) -> Option<std::net::IpAddr> {
    let h = req.headers();
    let v = h.get("cf-connecting-ip").or_else(|| h.get("x-forwarded-for"))?.to_str().ok()?;
    v.split(',').next()?.trim().parse().ok()
}

/// Aceita o WebSocket e liga as mensagens binárias a um fluxo de bytes comum,
/// que segue pelo mesmo caminho do protocolo direto.
// O tipo de erro do callback é o do tungstenite (resposta HTTP inteira).
#[allow(clippy::result_large_err)]
async fn websocket(state: Arc<State>, s: tokio::io::BufReader<BoxStream>, peer: SocketAddr) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let mut real = peer;
    let cb = |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
        if trusted_proxy(peer.ip())
            && let Some(ip) = forwarded_ip(req)
        {
            real = SocketAddr::new(ip, 0);
        }
        Ok(resp)
    };
    let Ok(Ok(ws)) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::accept_hdr_async(s, cb)).await else { return };
    let (mine, theirs) = tokio::io::duplex(256 * 1024);
    let pump = async move {
        let (mut sink, mut stream) = ws.split();
        let (mut rd, mut wr) = tokio::io::split(theirs);
        let to_ws = async {
            let mut buf = vec![0u8; 64 * 1024];
            // Ping periódico: proxies (Cloudflare) fecham WebSockets parados.
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.tick().await;
            loop {
                tokio::select! {
                    r = rd.read(&mut buf) => match r {
                        Ok(0) | Err(_) => break,
                        Ok(n) => if sink.send(Message::binary(buf[..n].to_vec())).await.is_err() { break },
                    },
                    _ = tick.tick() => if sink.send(Message::Ping(Vec::new().into())).await.is_err() { break },
                }
            }
            let _ = sink.close().await;
        };
        let from_ws = async {
            while let Some(Ok(m)) = stream.next().await {
                match m {
                    Message::Binary(b) => {
                        if wr.write_all(&b).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        };
        // Um lado terminou: descarta tudo, e o outro lado vê o fim da conexão.
        tokio::select! {
            _ = to_ws => {}
            _ = from_ws => {}
        }
    };
    let pump = tokio::spawn(pump);
    dispatch(state, Box::new(mine), real).await;
    // `mine` foi descartado: o pump termina sozinho ao ler o fim.
    let _ = tokio::time::timeout(Duration::from_secs(5), pump).await;
}

async fn dispatch(state: Arc<State>, mut s: BoxStream, peer: SocketAddr) {
    let Ok(Ok(first)) = tokio::time::timeout(Duration::from_secs(15), read_frame(&mut s)).await else { return };
    let Ok(msg) = postcard::from_bytes::<ToServer>(&first) else { return };
    match msg {
        ToServer::Hello { token, name, os, version, mac } => control(state, s, peer, token, name, os, version, mac).await,
        ToServer::Join { session, secret, host } => join(state, s, session, secret, host).await,
        _ => {
            let _ = write_msg(&mut s, &FromServer::Error { code: ErrorCode::BadRequest, message: "comece com Hello ou Join".into() }).await;
        }
    }
}

/// Como o servidor fala TLS com os apps.
#[derive(Debug, Clone)]
pub enum Tls {
    /// Sem TLS (só para testes).
    Off,
    /// Certificado autoassinado persistente em `data` (os apps fixam a impressão digital).
    SelfSigned,
    /// Certificado público (PEM).
    Files { cert: PathBuf, key: PathBuf },
}

#[derive(Debug, Clone)]
pub struct Config {
    /// "ip:porta" para escutar.
    pub listen: String,
    /// Pasta de dados (IDs, sessões, certificado autoassinado).
    pub data: PathBuf,
    pub tls: Tls,
    /// Dias que o histórico de sessões (`sessoes.jsonl`) é guardado; 0 = para sempre.
    pub retention_days: u32,
}

/// Impressão digital SHA-256 (hex) de um certificado DER.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// Impressão digital do certificado autoassinado já gravado em `data`, se houver.
pub fn self_signed_fingerprint(data: &Path) -> Option<String> {
    let f = std::fs::File::open(data.join("cert.pem")).ok()?;
    let cert = rustls_pemfile::certs(&mut std::io::BufReader::new(f)).filter_map(|x| x.ok()).next()?;
    Some(fingerprint(cert.as_ref()))
}

fn load_pem(cert: &Path, key: &Path) -> std::io::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let ctx = |what: &str, p: &Path, e: std::io::Error| std::io::Error::new(e.kind(), format!("{what} {}: {e}", p.display()));
    let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(std::fs::File::open(cert).map_err(|e| ctx("certificado", cert, e))?)).filter_map(|x| x.ok()).collect();
    if certs.is_empty() {
        return Err(std::io::Error::other(format!("nenhum certificado em {}", cert.display())));
    }
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(std::fs::File::open(key).map_err(|e| ctx("chave", key, e))?))
        .ok()
        .flatten()
        .ok_or_else(|| std::io::Error::other(format!("nenhuma chave privada em {}", key.display())))?;
    Ok((certs, key))
}

/// Monta o TLS. Devolve também a impressão digital (só para autoassinado).
fn tls_config(cfg: &Config) -> std::io::Result<Option<(Arc<rustls::ServerConfig>, Option<String>)>> {
    let (certs, key, fp) = match &cfg.tls {
        Tls::Off => return Ok(None),
        Tls::Files { cert, key } => {
            let (c, k) = load_pem(cert, key)?;
            (c, k, None)
        }
        Tls::SelfSigned => {
            let cp = cfg.data.join("cert.pem");
            let kp = cfg.data.join("key.pem");
            if !cp.exists() || !kp.exists() {
                let ck = rcgen::generate_simple_self_signed(vec!["elo".into(), "localhost".into()]).map_err(std::io::Error::other)?;
                std::fs::write(&cp, ck.cert.pem())?;
                std::fs::write(&kp, ck.key_pair.serialize_pem())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&kp, std::fs::Permissions::from_mode(0o600));
                }
            }
            let (c, k) = load_pem(&cp, &kp)?;
            let fp = fingerprint(c[0].as_ref());
            (c, k, Some(fp))
        }
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(std::io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(std::io::Error::other)?;
    Ok(Some((Arc::new(tls), fp)))
}

/// Servidor já escutando na porta, pronto para [`Server::run`].
pub struct Server {
    listener: TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
    state: Arc<State>,
    /// Endereço em que está escutando (útil com porta 0).
    pub local_addr: SocketAddr,
    /// Impressão digital do certificado autoassinado (para os apps fixarem).
    pub fingerprint: Option<String>,
}

impl Server {
    /// Prepara a pasta de dados, o TLS e abre a porta.
    pub async fn bind(cfg: Config) -> std::io::Result<Server> {
        std::fs::create_dir_all(&cfg.data)?;
        let ids: HashMap<String, String> = std::fs::read(cfg.data.join("ids.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let (tls, fingerprint) = match tls_config(&cfg)? {
            Some((t, fp)) => (Some(tokio_rustls::TlsAcceptor::from(t)), fp),
            None => (None, None),
        };
        let listener = TcpListener::bind(&cfg.listen).await.map_err(|e| std::io::Error::new(e.kind(), format!("porta {}: {e}", cfg.listen)))?;
        let local_addr = listener.local_addr()?;
        prune_log(&cfg.data, cfg.retention_days);
        let known: HashMap<String, Known> = std::fs::read(cfg.data.join("dispositivos.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let state = Arc::new(State {
            data: cfg.data,
            ids: Mutex::new(ids),
            online: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            limits: Mutex::new(Limits::default()),
            active: Mutex::new(HashMap::new()),
            known: Mutex::new(known),
            retention_days: std::sync::atomic::AtomicU32::new(cfg.retention_days),
        });
        Ok(Server { listener, tls, state, local_addr, fingerprint })
    }

    pub fn tls(&self) -> bool {
        self.tls.is_some()
    }

    /// Atende conexões até a future ser descartada.
    pub async fn run(self) -> std::io::Result<()> {
        let Server { listener, tls, state, .. } = self;
        // Limpeza de convites expirados.
        let st = state.clone();
        let cleanup = tokio::spawn(async move {
            let mut last_prune = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                // Retenção do histórico: uma vez por hora.
                if last_prune.elapsed() > Duration::from_secs(3600) {
                    last_prune = Instant::now();
                    prune_log(&st.data, st.retention_days.load(std::sync::atomic::Ordering::Relaxed));
                }
                st.pending.lock().await.retain(|_, p| p.created.elapsed() < Duration::from_secs(150));
                st.limits.lock().await.hits.retain(|_, v| v.last().is_some_and(|t| t.elapsed() < Duration::from_secs(60)));
            }
        });
        // Para a limpeza quando o servidor é descartado.
        struct Abort(tokio::task::JoinHandle<()>);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _cleanup = Abort(cleanup);
        loop {
            let (sock, peer) = match listener.accept().await {
                Ok(x) => x,
                // Erros passageiros (ex.: limite de arquivos abertos) não derrubam o servidor.
                Err(e) => {
                    eprintln!("aceitar conexão: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let _ = sock.set_nodelay(true);
            let state = state.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                let s: BoxStream = match tls {
                    Some(acc) => {
                        // TLS começa com 0x16 (handshake). Sem TLS, só do próprio
                        // computador (proxy local como o cloudflared).
                        let mut b = [0u8; 1];
                        let is_tls = matches!(tokio::time::timeout(Duration::from_secs(10), sock.peek(&mut b)).await, Ok(Ok(1))) && b[0] == 0x16;
                        if is_tls {
                            match tokio::time::timeout(Duration::from_secs(10), acc.accept(sock)).await {
                                Ok(Ok(t)) => Box::new(t),
                                _ => return,
                            }
                        } else if peer.ip().is_loopback() {
                            Box::new(sock)
                        } else {
                            return;
                        }
                    }
                    None => Box::new(sock),
                };
                handle(state, s, peer).await;
            });
        }
    }
}

/// Computador conectado ao servidor agora.
#[derive(Debug, Clone)]
pub struct OnlineDevice {
    pub id: String,
    pub name: String,
    pub os: String,
    pub ip: String,
    /// Conectado desde (segundos desde 1970).
    pub since: u64,
}

/// Sessão em andamento (`end == 0`) ou do histórico.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub host_id: String,
    pub host_name: String,
    pub viewer_id: String,
    pub viewer_name: String,
    pub start: u64,
    pub end: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub online: Vec<OnlineDevice>,
    pub active: Vec<SessionInfo>,
    /// Mais recentes primeiro (até 50).
    pub recent: Vec<SessionInfo>,
    pub retention_days: u32,
}

/// Servidor rodando numa thread própria (para apps sem tokio, como o Elo).
/// Para ao ser descartado.
pub struct Background {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    state: Arc<State>,
    pub local_addr: SocketAddr,
    pub fingerprint: Option<String>,
}

impl Background {
    /// Sobe o servidor e espera a porta abrir (erros de porta em uso aparecem aqui).
    pub fn start(cfg: Config) -> std::io::Result<Background> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<(SocketAddr, Option<String>, Arc<State>)>>();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let thread = std::thread::Builder::new().name("elo-servidor".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("elo-servidor-rt").enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            rt.block_on(async move {
                let server = match Server::bind(cfg).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok((server.local_addr, server.fingerprint.clone(), server.state.clone())));
                tokio::select! {
                    r = server.run() => if let Err(e) = r { eprintln!("servidor local parou: {e}") },
                    _ = stop_rx => {}
                }
            });
            // Derruba as conexões e sessões ainda abertas.
            rt.shutdown_timeout(Duration::from_secs(2));
        })?;
        match ready_rx.recv() {
            Ok(Ok((local_addr, fingerprint, state))) => Ok(Background { stop: Some(stop_tx), thread: Some(thread), state, local_addr, fingerprint }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(std::io::Error::other("o servidor local não iniciou"))
            }
        }
    }

    /// Ainda rodando (falso se a thread terminou por erro).
    pub fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Muda o prazo do histórico (vale na próxima limpeza, de hora em hora, e já limpa agora).
    pub fn set_retention_days(&self, days: u32) {
        self.state.retention_days.store(days, std::sync::atomic::Ordering::Relaxed);
        prune_log(&self.state.data, days);
    }

    /// Fotografia do servidor para o painel: quem está online, sessões em
    /// andamento e as últimas do histórico. Não chame de dentro do tokio.
    pub fn stats(&self) -> Stats {
        let st = &self.state;
        let names = st.known.blocking_lock().clone();
        let name = |id: &str| names.get(id).map(|x| x.name.clone()).unwrap_or_default();
        let mut online: Vec<OnlineDevice> = st.online.blocking_lock().iter().filter_map(|(id, v)| v.last().map(|d| OnlineDevice { id: id.clone(), name: d.name.clone(), os: d.os.clone(), ip: d.ip.clone(), since: d.since })).collect();
        online.sort_by_key(|d| d.name.to_lowercase());
        let mut active: Vec<SessionInfo> = st.active.blocking_lock().values().map(|(h, v, start)| SessionInfo { host_id: h.clone(), host_name: name(h), viewer_id: v.clone(), viewer_name: name(v), start: *start, end: 0, bytes: 0 }).collect();
        active.sort_by_key(|s| std::cmp::Reverse(s.start));
        let text = std::fs::read_to_string(st.data.join("sessoes.jsonl")).unwrap_or_default();
        let recent: Vec<SessionInfo> = text
            .lines()
            .rev()
            .take(50)
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .map(|v| {
                let h = v["anfitriao"].as_str().unwrap_or_default().to_string();
                let w = v["visualizador"].as_str().unwrap_or_default().to_string();
                SessionInfo { host_name: name(&h), viewer_name: name(&w), host_id: h, viewer_id: w, start: v["inicio"].as_u64().unwrap_or(0), end: v["fim"].as_u64().unwrap_or(0), bytes: v["bytes"].as_u64().unwrap_or(0) }
            })
            .collect();
        Stats { online, active, recent, retention_days: st.retention_days.load(std::sync::atomic::Ordering::Relaxed) }
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        // Espera liberar a porta (para religar na mesma porta logo em seguida).
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
