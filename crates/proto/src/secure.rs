//! Aperto de mão e canal seguro.
//!
//! O par troca mensagens SPAKE2 (Ed25519) derivadas da senha: quem não sabe a
//! senha — inclusive o servidor de relay — não obtém a chave nem consegue
//! intermediar sem ser detectado. As identidades do SPAKE2 levam o ID de
//! quem acessa, o ID do computador acessado e a sessão: o relay não consegue
//! desviar a conexão para outro computador que tenha a mesma senha.
//!
//! Sem senha (acesso com aceite), a chave é um acordo anônimo e os dois lados
//! mostram um código de verificação de 8 dígitos. Quem acessa se compromete
//! com a própria mensagem (hash) antes de ver a do anfitrião: quem estiver no
//! meio tem uma única chance em 100 milhões por tentativa de fazer os dois
//! códigos baterem, e cada tentativa abandonada conta no anfitrião. Técnicos
//! de empresa ainda provam quem são com o certificado (`cert`), assinando o
//! vínculo do canal.
//!
//! O anfitrião tenta até duas senhas (temporária e permanente), cada uma num
//! SPAKE2 novo; a confirmação por HMAC revela qual funcionou sem expor a senha.

use std::io::{Read, Write};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::frame::{read_frame, read_frame_max, write_frame};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMode {
    /// Senha (temporária ou de acesso não assistido).
    Password,
    /// Sem senha: o anfitrião precisa aceitar na tela.
    Request,
}

#[derive(Debug, Serialize, Deserialize)]
enum Hs {
    /// `commit`: no modo sem senha, SHA-256 da mensagem SPAKE2 de quem acessa.
    Start { mode: AuthMode, version: u32, commit: Vec<u8> },
    PakeB { attempt: u8, msg: Vec<u8> },
    PakeA { attempt: u8, msg: Vec<u8> },
    ConfirmA { attempt: u8, mac: Vec<u8> },
    Retry { attempt: u8 },
    ConfirmB { mac: Vec<u8> },
    Fail,
    /// Sem senha, depois da confirmação: certificado do técnico e assinatura
    /// do vínculo da sessão (vazios para quem não é técnico).
    Proof { cert: Vec<u8>, sig: Vec<u8> },
}

/// Quem está nas duas pontas, como cada lado sabe.
#[derive(Debug, Clone, Copy)]
pub struct Peers<'a> {
    pub session: u64,
    /// ID de quem acessa.
    pub viewer: &'a str,
    /// ID do computador acessado (o que quem acessa digitou).
    pub host: &'a str,
}

impl Peers<'_> {
    fn ids(&self) -> (Vec<u8>, Vec<u8>) {
        (format!("elo-visualizador:{}", self.viewer).into_bytes(), format!("elo-anfitriao:{}:{:x}", self.host, self.session).into_bytes())
    }
}

/// Técnico de empresa: chave secreta e certificado (`cert::SignedCert`).
#[derive(Debug, Clone)]
pub struct Credential {
    pub secret: [u8; 32],
    pub cert: Vec<u8>,
}

fn mac(key: &[u8], label: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac");
    m.update(label);
    m.finalize().into_bytes().to_vec()
}

fn send_hs(w: &mut impl Write, m: &Hs) -> Result<()> {
    write_frame(w, &postcard::to_stdvec(m)?)
}

fn recv_hs(r: &mut impl Read) -> Result<Hs> {
    Ok(postcard::from_bytes(&read_frame_max(r, 4096)?)?)
}

/// Chaves da sessão, já confirmadas.
pub struct Keys {
    pub send: [u8; 32],
    pub recv: [u8; 32],
    /// Código de verificação (8 dígitos) igual nos dois lados.
    pub sas: String,
    pub mode: AuthMode,
    /// Qual senha do anfitrião funcionou (0 = primeira, 1 = segunda).
    pub which: u8,
    /// Vínculo do canal (igual nos dois lados só sem ninguém no meio).
    pub binding: [u8; 32],
    /// Anfitrião, sem senha: o que quem acessa apresentou (certificado, assinatura).
    pub proof: Option<(Vec<u8>, Vec<u8>)>,
}

fn derive(shared: &[u8], viewer: bool, mode: AuthMode, which: u8) -> Keys {
    let hk = Hkdf::<Sha256>::new(Some(b"elo-v3"), shared);
    let mut a2b = [0u8; 32];
    let mut b2a = [0u8; 32];
    let mut sas = [0u8; 8];
    let mut binding = [0u8; 32];
    hk.expand(b"viewer->host", &mut a2b).expect("hkdf");
    hk.expand(b"host->viewer", &mut b2a).expect("hkdf");
    hk.expand(b"sas", &mut sas).expect("hkdf");
    hk.expand(b"vinculo", &mut binding).expect("hkdf");
    let code = u64::from_le_bytes(sas) % 100_000_000;
    let (send, recv) = if viewer { (a2b, b2a) } else { (b2a, a2b) };
    Keys { send, recv, sas: format!("{:04} {:04}", code / 10_000, code % 10_000), mode, which, binding, proof: None }
}

/// Rodadas de SPAKE2 num aperto de mão (senha temporária e senha fixa).
const MAX_ROUNDS: u8 = 2;

/// Lado de quem acessa. `password` vazio = pedir acesso (aceite na tela).
/// `credential`: técnico de empresa (prova quem é ao pedir acesso).
pub fn viewer_handshake(s: &mut (impl Read + Write), password: &str, peers: Peers, credential: Option<&Credential>) -> Result<Keys> {
    let mode = if password.is_empty() { AuthMode::Request } else { AuthMode::Password };
    let pw = Password::new(password.as_bytes());
    let (id_a, id_b) = peers.ids();
    // Sem senha: a mensagem é escolhida antes de ver a do anfitrião, e o
    // compromisso (hash) vai já no início.
    let mut first = (mode == AuthMode::Request).then(|| Spake2::<Ed25519Group>::start_a(&pw, &Identity::new(&id_a), &Identity::new(&id_b)));
    let commit = first.as_ref().map(|(_, ma)| Sha256::digest(ma).to_vec()).unwrap_or_default();
    send_hs(s, &Hs::Start { mode, version: crate::PROTOCOL_VERSION, commit })?;
    // O anfitrião tenta no máximo duas senhas. Mais rodadas seriam alguém
    // (relay ou falso anfitrião) usando esta conexão para adivinhar a nossa.
    for round in 0..MAX_ROUNDS {
        let Hs::PakeB { attempt, msg: mb } = recv_hs(s)? else { return Err(Error::Auth) };
        if attempt != round || (mode == AuthMode::Request && round > 0) {
            return Err(Error::Auth);
        }
        let (st, ma) = first.take().unwrap_or_else(|| Spake2::<Ed25519Group>::start_a(&pw, &Identity::new(&id_a), &Identity::new(&id_b)));
        send_hs(s, &Hs::PakeA { attempt, msg: ma })?;
        let k = st.finish(&mb).map_err(|_| Error::Auth)?;
        send_hs(s, &Hs::ConfirmA { attempt, mac: mac(&k, b"confirm-a") })?;
        match recv_hs(s)? {
            Hs::ConfirmB { mac: m } => {
                if m != mac(&k, b"confirm-b") {
                    return Err(Error::Auth);
                }
                let keys = derive(&k, true, mode, attempt);
                if mode == AuthMode::Request {
                    let proof = match credential {
                        Some(c) => Hs::Proof { cert: c.cert.clone(), sig: crate::cert::prove(&c.secret, &keys.binding) },
                        None => Hs::Proof { cert: Vec::new(), sig: Vec::new() },
                    };
                    send_hs(s, &proof)?;
                }
                return Ok(keys);
            }
            Hs::Retry { .. } => continue,
            _ => return Err(Error::Auth),
        }
    }
    Err(Error::Auth)
}

/// Lado de quem é acessado. `passwords`: senhas aceitas, na ordem de tentativa.
/// Com o modo "pedir acesso", usa a senha vazia (o app exige o aceite), a
/// menos que `allow_request` seja falso (tentativas demais abandonadas).
pub fn host_handshake(s: &mut (impl Read + Write), passwords: &[String], peers: Peers, allow_request: bool) -> Result<Keys> {
    let Hs::Start { mode, version, commit } = recv_hs(s)? else { return Err(Error::Auth) };
    if version != crate::PROTOCOL_VERSION {
        let _ = send_hs(s, &Hs::Fail);
        return Err(Error::Other(format!("versão do protocolo diferente ({version})")));
    }
    let list: Vec<String> = match mode {
        AuthMode::Request if allow_request && commit.len() == 32 => vec![String::new()],
        AuthMode::Request => Vec::new(),
        AuthMode::Password => passwords.iter().filter(|p| !p.is_empty()).cloned().collect(),
    };
    if list.is_empty() {
        let _ = send_hs(s, &Hs::Fail);
        return Err(Error::Auth);
    }
    let (id_a, id_b) = peers.ids();
    // Depois de uma senha errada, desistir no meio também conta como erro:
    // senão bastava desconectar após a primeira tentativa para nunca bloquear.
    // Sem senha, desistir depois de ver a nossa mensagem conta como tentativa
    // abandonada: é o que alguém no meio faria para tentar de novo o código.
    let request = mode == AuthMode::Request;
    let mut wrong = false;
    let after = |wrong: bool| move |e: Error| if request { Error::Abandoned } else if wrong { Error::Auth } else { e };
    for (attempt, p) in list.iter().enumerate() {
        let attempt = attempt as u8;
        let (st, mb) = Spake2::<Ed25519Group>::start_b(&Password::new(p.as_bytes()), &Identity::new(&id_a), &Identity::new(&id_b));
        send_hs(s, &Hs::PakeB { attempt, msg: mb })?;
        let Hs::PakeA { msg: ma, .. } = recv_hs(s).map_err(after(wrong))? else { return Err(Error::Auth) };
        if request && Sha256::digest(&ma).as_slice() != commit.as_slice() {
            let _ = send_hs(s, &Hs::Fail);
            return Err(Error::Abandoned);
        }
        let Hs::ConfirmA { mac: m, .. } = recv_hs(s).map_err(after(wrong))? else { return Err(Error::Auth) };
        let Ok(k) = st.finish(&ma) else { return Err(Error::Auth) };
        if m == mac(&k, b"confirm-a") {
            send_hs(s, &Hs::ConfirmB { mac: mac(&k, b"confirm-b") })?;
            let mut keys = derive(&k, false, mode, attempt);
            if request {
                let Hs::Proof { cert, sig } = recv_hs(s).map_err(after(wrong))? else { return Err(Error::Auth) };
                keys.proof = (!cert.is_empty()).then_some((cert, sig));
            }
            return Ok(keys);
        }
        wrong = true;
        if (attempt as usize) + 1 < list.len() {
            send_hs(s, &Hs::Retry { attempt }).map_err(after(wrong))?;
        }
    }
    let _ = send_hs(s, &Hs::Fail);
    Err(if request { Error::Abandoned } else { Error::Auth })
}

/// Cifra um sentido do canal. O contador vira o nonce (nunca repete).
pub struct Sealer {
    aead: ChaCha20Poly1305,
    counter: u64,
}

pub struct Opener {
    aead: ChaCha20Poly1305,
    counter: u64,
}

fn nonce(c: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&c.to_le_bytes());
    *Nonce::from_slice(&n)
}

impl Sealer {
    pub fn new(key: &[u8; 32]) -> Self {
        Self { aead: ChaCha20Poly1305::new(Key::from_slice(key)), counter: 0 }
    }

    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        let n = nonce(self.counter);
        self.counter += 1;
        self.aead.encrypt(&n, Payload { msg: plain, aad: b"elo" }).map_err(|_| Error::Crypto)
    }

    pub fn send<T: Serialize>(&mut self, w: &mut impl Write, msg: &T) -> Result<()> {
        let data = self.seal(&postcard::to_stdvec(msg)?)?;
        write_frame(w, &data)
    }
}

impl Opener {
    pub fn new(key: &[u8; 32]) -> Self {
        Self { aead: ChaCha20Poly1305::new(Key::from_slice(key)), counter: 0 }
    }

    pub fn open(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let n = nonce(self.counter);
        self.counter += 1;
        self.aead.decrypt(&n, Payload { msg: data, aad: b"elo" }).map_err(|_| Error::Crypto)
    }

    pub fn recv<T: serde::de::DeserializeOwned>(&mut self, r: &mut impl Read) -> Result<T> {
        let data = read_frame(r)?;
        Ok(postcard::from_bytes(&self.open(&data)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    /// Par de sockets conectados (TCP local: funciona em todos os sistemas).
    fn socket_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (b, _) = l.accept().unwrap();
        (a, b)
    }

    const PEERS: Peers<'static> = Peers { session: 0xabc, viewer: "111222333", host: "444555666" };

    fn pair_with(host_pw: Vec<String>, viewer_pw: &str, host_peers: Peers<'static>, cred: Option<Credential>) -> (Result<Keys>, Result<Keys>) {
        let (mut a, mut b) = socket_pair();
        let h = std::thread::spawn(move || host_handshake(&mut b, &host_pw, host_peers, true));
        let v = viewer_handshake(&mut a, viewer_pw, PEERS, cred.as_ref());
        (v, h.join().unwrap())
    }

    fn pair(host_pw: Vec<String>, viewer_pw: &str) -> (Result<Keys>, Result<Keys>) {
        pair_with(host_pw, viewer_pw, PEERS, None)
    }

    #[test]
    fn password_and_second_password_and_failure() {
        let (v, h) = pair(vec!["temp123".into(), "fixa-longa".into()], "temp123");
        let (v, h) = (v.unwrap(), h.unwrap());
        assert_eq!(v.send, h.recv);
        assert_eq!(v.sas, h.sas);
        assert_eq!(h.which, 0);
        let (v, h) = pair(vec!["temp123".into(), "fixa-longa".into()], "fixa-longa");
        assert_eq!(h.unwrap().which, 1);
        assert!(v.is_ok());
        let (v, h) = pair(vec!["temp123".into()], "errada");
        assert!(v.is_err() && h.is_err());
        // Pedido sem senha: código de 8 dígitos igual dos dois lados.
        let (v, h) = pair(vec!["temp123".into()], "");
        let (v, h) = (v.unwrap(), h.unwrap());
        assert_eq!(h.mode, AuthMode::Request);
        assert_eq!(v.sas, h.sas);
        assert_eq!(v.binding, h.binding);
        assert!(v.sas.len() == 9 && v.sas.chars().filter(|c| c.is_ascii_digit()).count() == 8, "{}", v.sas);
        assert!(h.proof.is_none());
    }

    #[test]
    fn same_password_on_another_computer_does_not_open() {
        // O relay desvia para outro computador (outro ID) com a mesma senha.
        let other = Peers { host: "999888777", ..PEERS };
        let (v, h) = pair_with(vec!["mesma-senha".into()], "mesma-senha", other, None);
        assert!(v.is_err() && h.is_err());
        let other_session = Peers { session: 0xdef, ..PEERS };
        let (v, h) = pair_with(vec!["mesma-senha".into()], "mesma-senha", other_session, None);
        assert!(v.is_err() && h.is_err());
    }

    #[test]
    fn tech_proof_reaches_the_host_bound_to_this_session() {
        let ca = crate::cert::new_secret();
        let tech = crate::cert::new_secret();
        let cert = crate::cert::issue(&ca, crate::cert::TechCert { user: "ana".into(), name: "Ana".into(), key: crate::cert::public_of(&tech), expires: u64::MAX });
        let cred = Credential { secret: tech, cert: cert.to_bytes() };
        let (v, h) = pair_with(Vec::new(), "", PEERS, Some(cred));
        let (v, h) = (v.unwrap(), h.unwrap());
        let (c, sig) = h.proof.clone().expect("prova");
        let ok = crate::cert::check(&crate::cert::public_of(&ca), 0, &c, &sig, &h.binding);
        assert_eq!(ok.map(|c| c.user), Some("ana".into()));
        assert_eq!(v.binding, h.binding);
    }

    #[test]
    fn request_without_commitment_or_with_another_message_is_refused() {
        // Quem está no meio mostra uma mensagem e depois revela outra.
        let (a, mut b) = socket_pair();
        let host = std::thread::spawn(move || {
            let mut a = a;
            host_handshake(&mut a, &[], PEERS, true)
        });
        let (ida, idb) = PEERS.ids();
        let (_, ma1) = Spake2::<Ed25519Group>::start_a(&Password::new(b""), &Identity::new(&ida), &Identity::new(&idb));
        let (_, ma2) = Spake2::<Ed25519Group>::start_a(&Password::new(b""), &Identity::new(&ida), &Identity::new(&idb));
        send_hs(&mut b, &Hs::Start { mode: AuthMode::Request, version: crate::PROTOCOL_VERSION, commit: Sha256::digest(&ma1).to_vec() }).unwrap();
        let _ = recv_hs(&mut b).unwrap();
        send_hs(&mut b, &Hs::PakeA { attempt: 0, msg: ma2 }).unwrap();
        assert!(matches!(host.join().unwrap(), Err(Error::Abandoned)));
        // Sem compromisso nenhum: recusado de cara.
        let (a, mut b) = socket_pair();
        let host = std::thread::spawn(move || {
            let mut a = a;
            host_handshake(&mut a, &[], PEERS, true)
        });
        send_hs(&mut b, &Hs::Start { mode: AuthMode::Request, version: crate::PROTOCOL_VERSION, commit: Vec::new() }).unwrap();
        assert!(matches!(recv_hs(&mut b).unwrap(), Hs::Fail));
        assert!(matches!(host.join().unwrap(), Err(Error::Auth)));
    }

    #[test]
    fn giving_up_after_seeing_the_host_message_counts() {
        let (a, mut b) = socket_pair();
        let host = std::thread::spawn(move || {
            let mut a = a;
            host_handshake(&mut a, &[], PEERS, true)
        });
        send_hs(&mut b, &Hs::Start { mode: AuthMode::Request, version: crate::PROTOCOL_VERSION, commit: vec![1; 32] }).unwrap();
        assert!(matches!(recv_hs(&mut b).unwrap(), Hs::PakeB { .. }));
        drop(b);
        assert!(matches!(host.join().unwrap(), Err(Error::Abandoned)));
        // Pedidos bloqueados: nem começa.
        let (mut a, mut b) = socket_pair();
        let host = std::thread::spawn(move || host_handshake(&mut b, &["x".into()], PEERS, false));
        assert!(viewer_handshake(&mut a, "", PEERS, None).is_err());
        assert!(host.join().unwrap().is_err());
    }

    #[test]
    fn viewer_stops_answering_after_two_rounds() {
        let (mut a, mut b) = socket_pair();
        let fake_host = std::thread::spawn(move || {
            let _ = recv_hs(&mut b);
            let (ida, idb) = PEERS.ids();
            let mut rounds = 0u8;
            loop {
                let (_, msg) = Spake2::<Ed25519Group>::start_b(&Password::new(b"palpite"), &Identity::new(&ida), &Identity::new(&idb));
                if send_hs(&mut b, &Hs::PakeB { attempt: rounds, msg }).is_err() || recv_hs(&mut b).is_err() || recv_hs(&mut b).is_err() {
                    return rounds;
                }
                rounds += 1;
                let _ = send_hs(&mut b, &Hs::Retry { attempt: rounds - 1 });
            }
        });
        assert!(matches!(viewer_handshake(&mut a, "senha-certa", PEERS, None), Err(Error::Auth)));
        drop(a);
        assert_eq!(fake_host.join().unwrap(), MAX_ROUNDS);
    }

    #[test]
    fn giving_up_after_a_wrong_password_counts_as_wrong() {
        let (a, mut b) = socket_pair();
        let host = std::thread::spawn(move || {
            let mut a = a;
            host_handshake(&mut a, &["certa".into(), "outra".into()], PEERS, true)
        });
        send_hs(&mut b, &Hs::Start { mode: AuthMode::Password, version: crate::PROTOCOL_VERSION, commit: Vec::new() }).unwrap();
        let Hs::PakeB { msg: mb, .. } = recv_hs(&mut b).unwrap() else { panic!() };
        let (ida, idb) = PEERS.ids();
        let (st, ma) = Spake2::<Ed25519Group>::start_a(&Password::new(b"errada"), &Identity::new(&ida), &Identity::new(&idb));
        let k = st.finish(&mb).unwrap();
        send_hs(&mut b, &Hs::PakeA { attempt: 0, msg: ma }).unwrap();
        send_hs(&mut b, &Hs::ConfirmA { attempt: 0, mac: mac(&k, b"confirm-a") }).unwrap();
        assert!(matches!(recv_hs(&mut b).unwrap(), Hs::Retry { .. }));
        drop(b);
        assert!(matches!(host.join().unwrap(), Err(Error::Auth)));
    }

    #[test]
    fn channel_roundtrip_and_tamper() {
        let k = [7u8; 32];
        let mut s = Sealer::new(&k);
        let mut o = Opener::new(&k);
        let c = s.seal(b"ola").unwrap();
        assert_eq!(o.open(&c).unwrap(), b"ola");
        let mut c2 = s.seal(b"de novo").unwrap();
        c2[0] ^= 1;
        assert!(o.open(&c2).is_err());
    }
}
