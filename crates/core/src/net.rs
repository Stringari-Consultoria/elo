//! Conexão com o servidor (TCP, com TLS por padrão, ou WebSocket para passar
//! por proxies como o Cloudflare Tunnel) e leitura de quadros sem bloquear
//! (para alternar envio e recebimento na mesma thread).

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServerCfg {
    /// "host:porta", ou "wss://dominio[/caminho]" (WebSocket, ex.: Cloudflare Tunnel).
    pub address: String,
    pub tls: bool,
    /// Impressão digital SHA-256 (hex) de certificado autoassinado; vazio = cadeia pública.
    pub pin: String,
}

impl Default for ServerCfg {
    /// Sem servidor externo: o app usa o servidor embutido até alguém preencher.
    fn default() -> Self {
        Self { address: String::new(), tls: true, pin: String::new() }
    }
}

pub enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
    Ws(Box<Ws>),
}

/// WebSocket como fluxo de bytes: cada `flush` vira mensagens binárias (de
/// até 64 KB); as mensagens recebidas são lidas em sequência.
pub struct Ws {
    ws: tungstenite::WebSocket<Conn>,
    rd: Vec<u8>,
    pos: usize,
    wr: Vec<u8>,
}

const WS_CHUNK: usize = 64 * 1024;

impl Ws {
    fn send_chunks(&mut self, all: bool) -> std::io::Result<()> {
        while self.wr.len() >= WS_CHUNK || (all && !self.wr.is_empty()) {
            let n = self.wr.len().min(WS_CHUNK);
            let chunk: Vec<u8> = self.wr.drain(..n).collect();
            self.ws.write(tungstenite::Message::binary(chunk)).map_err(ws_err)?;
        }
        Ok(())
    }
}

fn ws_err(e: tungstenite::Error) -> std::io::Error {
    match e {
        tungstenite::Error::Io(e) => e,
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => std::io::Error::new(ErrorKind::ConnectionAborted, "conexão encerrada"),
        e => std::io::Error::other(e.to_string()),
    }
}

impl Conn {
    pub fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s,
            Conn::Tls(t) => t.get_ref(),
            Conn::Ws(w) => w.ws.get_ref().tcp(),
        }
    }

    pub fn set_read_timeout(&self, d: Option<Duration>) {
        let _ = self.tcp().set_read_timeout(d);
    }

    pub fn set_write_timeout(&self, d: Option<Duration>) {
        let _ = self.tcp().set_write_timeout(d);
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(t) => t.read(buf),
            Conn::Ws(w) => loop {
                if w.pos < w.rd.len() {
                    let n = buf.len().min(w.rd.len() - w.pos);
                    buf[..n].copy_from_slice(&w.rd[w.pos..w.pos + n]);
                    w.pos += n;
                    return Ok(n);
                }
                // Timeouts (WouldBlock/TimedOut) voltam como erro e podem ser
                // repetidos: o tungstenite guarda o que já leu.
                match w.ws.read() {
                    Ok(tungstenite::Message::Binary(b)) => {
                        w.rd = b.into();
                        w.pos = 0;
                    }
                    Ok(tungstenite::Message::Close(_)) => return Ok(0),
                    Ok(_) => {}
                    Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok(0),
                    Err(e) => return Err(ws_err(e)),
                }
            },
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(t) => t.write(buf),
            Conn::Ws(w) => {
                w.wr.extend_from_slice(buf);
                w.send_chunks(false)?;
                Ok(buf.len())
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(t) => t.flush(),
            Conn::Ws(w) => {
                w.send_chunks(true)?;
                w.ws.flush().map_err(ws_err)
            }
        }
    }
}

/// Aceita o certificado cuja impressão digital confere (servidores próprios autoassinados).
#[derive(Debug)]
struct Pinned(Vec<u8>, Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(&self, end: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if Sha256::digest(end.as_ref()).as_slice() == self.0.as_slice() {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("certificado do servidor não confere com a impressão digital configurada".into()))
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.1.signature_verification_algorithms.supported_schemes()
    }
}

fn hex(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (s.len() == 64).then(|| (0..32).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or(0)).collect())
}

pub fn connect(cfg: &ServerCfg) -> Result<Conn, String> {
    let address = cfg.address.trim();
    if address.is_empty() {
        return Err("nenhum servidor configurado: em Configurações → Servidor, ligue o servidor deste computador ou preencha o endereço".into());
    }
    let ws = address.strip_prefix("wss://").map(|r| (true, r)).or_else(|| address.strip_prefix("ws://").map(|r| (false, r)));
    let Some((secure, rest)) = ws else {
        let addr = if address.contains(':') { address.to_string() } else { format!("{address}:{}", elo_proto::DEFAULT_PORT) };
        return transport(&addr, cfg.tls, &cfg.pin);
    };
    // WebSocket: wss://host[:porta][/caminho]. Com wss o TLS é o do proxy
    // (certificado público), a não ser que haja impressão digital.
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("endereço inválido: {address}"));
    }
    let has_port = authority.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !authority.ends_with(']');
    let addr = if has_port { authority.to_string() } else { format!("{authority}:{}", if secure { 443 } else { 80 }) };
    let conn = transport(&addr, secure, &cfg.pin)?;
    match tungstenite::client::client(address, conn) {
        Ok((ws, _)) => Ok(Conn::Ws(Box::new(Ws { ws, rd: Vec::new(), pos: 0, wr: Vec::new() }))),
        Err(tungstenite::HandshakeError::Failure(e)) => Err(format!("WebSocket com {address}: {e}")),
        Err(tungstenite::HandshakeError::Interrupted(_)) => Err(format!("WebSocket com {address}: tempo esgotado")),
    }
}

/// TCP (e TLS, se pedido) até "host:porta".
fn transport(addr: &str, tls: bool, pin: &str) -> Result<Conn, String> {
    let tcp = {
        use std::net::ToSocketAddrs;
        let addrs: Vec<_> = addr.to_socket_addrs().map_err(|e| format!("servidor {addr}: {e}"))?.collect();
        let mut last = String::from("sem endereço");
        let mut ok = None;
        for a in addrs {
            match TcpStream::connect_timeout(&a, Duration::from_secs(8)) {
                Ok(s) => {
                    ok = Some(s);
                    break;
                }
                Err(e) => last = e.to_string(),
            }
        }
        ok.ok_or_else(|| format!("não foi possível conectar a {addr}: {last}"))?
    };
    let _ = tcp.set_nodelay(true);
    // Aperto de mão (TLS e WebSocket) com prazo; depois cada uso define o seu.
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(15)));
    if !tls {
        return Ok(Conn::Plain(tcp));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(|e| e.to_string())?;
    let config = match hex(pin) {
        Some(fp) => builder.dangerous().with_custom_certificate_verifier(Arc::new(Pinned(fp, provider))).with_no_client_auth(),
        None => {
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    let host = addr.rsplit_once(':').map(|x| x.0).unwrap_or(addr).trim_matches(['[', ']']).to_string();
    let name = ServerName::try_from(host.clone()).unwrap_or_else(|_| ServerName::try_from("elo".to_string()).expect("nome"));
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let mut s = rustls::StreamOwned::new(conn, tcp);
    // Força o aperto de mão TLS agora (erros de certificado aparecem aqui).
    s.flush().map_err(|e| format!("TLS com {addr}: {e}"))?;
    while s.conn.is_handshaking() {
        s.conn.complete_io(&mut s.sock).map_err(|e| format!("TLS com {addr}: {e}"))?;
    }
    Ok(Conn::Tls(Box::new(s)))
}

/// Leitor de quadros que tolera timeouts no meio de um quadro.
pub struct Framer {
    buf: Vec<u8>,
    /// Maior quadro aceito (menor enquanto o outro lado não foi aceito).
    max: usize,
}

impl Default for Framer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framer {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(1 << 16), max: elo_proto::frame::MAX_FRAME }
    }

    /// Muda o maior quadro aceito.
    pub fn set_max(&mut self, max: usize) {
        self.max = max;
    }

    /// Próximo quadro completo, `Ok(None)` se ainda não chegou.
    pub fn poll(&mut self, r: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(f) = self.take()? {
                return Ok(Some(f));
            }
            let mut tmp = [0u8; 64 * 1024];
            match r.read(&mut tmp) {
                Ok(0) => return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "conexão encerrada")),
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => return self.take(),
                Err(e) => return Err(e),
            }
        }
    }

    fn take(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let n = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        // Sem isto, um tamanho de 4 GiB fazia a memória crescer até lá.
        if n > self.max {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "o outro lado mandou um bloco grande demais"));
        }
        if self.buf.len() < 4 + n {
            return Ok(None);
        }
        let f = self.buf[4..4 + n].to_vec();
        self.buf.drain(..4 + n);
        Ok(Some(f))
    }
}

#[cfg(test)]
mod tests {
    use super::Framer;

    #[test]
    fn oversized_frame_is_an_error_not_a_wait() {
        let mut fr = Framer::new();
        fr.set_max(1024);
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(b"abc");
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(&[0; 100]);
        let mut r = &data[..];
        assert_eq!(fr.poll(&mut r).unwrap().as_deref(), Some(&b"abc"[..]));
        assert!(fr.poll(&mut r).is_err());
    }
}
