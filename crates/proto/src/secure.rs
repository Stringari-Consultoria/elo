//! Aperto de mão e canal seguro.
//!
//! O par troca mensagens SPAKE2 (Ed25519) derivadas da senha: quem não sabe a
//! senha — inclusive o servidor de relay — não obtém a chave nem consegue
//! intermediar sem ser detectado. Sem senha (acesso com aceite), a chave é
//! um acordo anônimo e os dois lados mostram um código de verificação de 6
//! dígitos para conferir.
//!
//! O anfitrião tenta até duas senhas (temporária e permanente), cada uma num
//! SPAKE2 novo; a confirmação por HMAC revela qual funcionou sem expor a senha.

use std::io::{Read, Write};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
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
    Start { mode: AuthMode, version: u32 },
    PakeB { attempt: u8, msg: Vec<u8> },
    PakeA { attempt: u8, msg: Vec<u8> },
    ConfirmA { attempt: u8, mac: Vec<u8> },
    Retry { attempt: u8 },
    ConfirmB { mac: Vec<u8> },
    Fail,
}

const ID_A: &[u8] = b"elo-visualizador";
const ID_B: &[u8] = b"elo-anfitriao";

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
    /// Código de verificação (6 dígitos) igual nos dois lados.
    pub sas: String,
    pub mode: AuthMode,
    /// Qual senha do anfitrião funcionou (0 = primeira, 1 = segunda).
    pub which: u8,
}

fn derive(shared: &[u8], viewer: bool, mode: AuthMode, which: u8) -> Keys {
    let hk = Hkdf::<Sha256>::new(Some(b"elo-v1"), shared);
    let mut a2b = [0u8; 32];
    let mut b2a = [0u8; 32];
    let mut sas = [0u8; 4];
    hk.expand(b"viewer->host", &mut a2b).expect("hkdf");
    hk.expand(b"host->viewer", &mut b2a).expect("hkdf");
    hk.expand(b"sas", &mut sas).expect("hkdf");
    let code = u32::from_le_bytes(sas) % 1_000_000;
    let (send, recv) = if viewer { (a2b, b2a) } else { (b2a, a2b) };
    Keys { send, recv, sas: format!("{:03} {:03}", code / 1000, code % 1000), mode, which }
}

/// Rodadas de SPAKE2 num aperto de mão (senha temporária e senha fixa).
const MAX_ROUNDS: u8 = 2;

/// Lado de quem acessa. `password` vazio = pedir acesso (aceite na tela).
pub fn viewer_handshake(s: &mut (impl Read + Write), password: &str) -> Result<Keys> {
    let mode = if password.is_empty() { AuthMode::Request } else { AuthMode::Password };
    send_hs(s, &Hs::Start { mode, version: crate::PROTOCOL_VERSION })?;
    let pw = Password::new(password.as_bytes());
    // O anfitrião tenta no máximo duas senhas. Mais rodadas seriam alguém
    // (relay ou falso anfitrião) usando esta conexão para adivinhar a nossa.
    for round in 0..MAX_ROUNDS {
        let Hs::PakeB { attempt, msg: mb } = recv_hs(s)? else { return Err(Error::Auth) };
        if attempt != round {
            return Err(Error::Auth);
        }
        let (st, ma) = Spake2::<Ed25519Group>::start_a(&pw, &Identity::new(ID_A), &Identity::new(ID_B));
        send_hs(s, &Hs::PakeA { attempt, msg: ma })?;
        let k = st.finish(&mb).map_err(|_| Error::Auth)?;
        send_hs(s, &Hs::ConfirmA { attempt, mac: mac(&k, b"confirm-a") })?;
        match recv_hs(s)? {
            Hs::ConfirmB { mac: m } => {
                if m != mac(&k, b"confirm-b") {
                    return Err(Error::Auth);
                }
                return Ok(derive(&k, true, mode, attempt));
            }
            Hs::Retry { .. } => continue,
            _ => return Err(Error::Auth),
        }
    }
    Err(Error::Auth)
}

/// Lado de quem é acessado. `passwords`: senhas aceitas, na ordem de tentativa.
/// Com o modo "pedir acesso", usa a senha vazia (o app exige o aceite).
pub fn host_handshake(s: &mut (impl Read + Write), passwords: &[String]) -> Result<Keys> {
    let Hs::Start { mode, version } = recv_hs(s)? else { return Err(Error::Auth) };
    if version != crate::PROTOCOL_VERSION {
        let _ = send_hs(s, &Hs::Fail);
        return Err(Error::Other(format!("versão do protocolo diferente ({version})")));
    }
    let list: Vec<String> = match mode {
        AuthMode::Request => vec![String::new()],
        AuthMode::Password => passwords.iter().filter(|p| !p.is_empty()).cloned().collect(),
    };
    if list.is_empty() {
        let _ = send_hs(s, &Hs::Fail);
        return Err(Error::Auth);
    }
    // Depois de uma senha errada, desistir no meio também conta como erro:
    // senão bastava desconectar após a primeira tentativa para nunca bloquear.
    let mut wrong = false;
    let after = |wrong: bool| move |e: Error| if wrong { Error::Auth } else { e };
    for (attempt, p) in list.iter().enumerate() {
        let attempt = attempt as u8;
        let (st, mb) = Spake2::<Ed25519Group>::start_b(&Password::new(p.as_bytes()), &Identity::new(ID_A), &Identity::new(ID_B));
        send_hs(s, &Hs::PakeB { attempt, msg: mb }).map_err(after(wrong))?;
        let Hs::PakeA { msg: ma, .. } = recv_hs(s).map_err(after(wrong))? else { return Err(Error::Auth) };
        let Hs::ConfirmA { mac: m, .. } = recv_hs(s).map_err(after(wrong))? else { return Err(Error::Auth) };
        let Ok(k) = st.finish(&ma) else { return Err(Error::Auth) };
        if m == mac(&k, b"confirm-a") {
            send_hs(s, &Hs::ConfirmB { mac: mac(&k, b"confirm-b") })?;
            return Ok(derive(&k, false, mode, attempt));
        }
        wrong = true;
        if (attempt as usize) + 1 < list.len() {
            send_hs(s, &Hs::Retry { attempt }).map_err(after(wrong))?;
        }
    }
    let _ = send_hs(s, &Hs::Fail);
    Err(Error::Auth)
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

    fn pair(host_pw: Vec<String>, viewer_pw: &str) -> (Result<Keys>, Result<Keys>) {
        let (mut a, mut b) = socket_pair();
        let h = std::thread::spawn(move || host_handshake(&mut b, &host_pw));
        let v = viewer_handshake(&mut a, viewer_pw);
        (v, h.join().unwrap())
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
        // Pedido sem senha.
        let (v, h) = pair(vec!["temp123".into()], "");
        let (v, h) = (v.unwrap(), h.unwrap());
        assert_eq!(h.mode, AuthMode::Request);
        assert_eq!(v.sas, h.sas);
    }

    #[test]
    fn viewer_stops_answering_after_two_rounds() {
        let (mut a, mut b) = socket_pair();
        let fake_host = std::thread::spawn(move || {
            let _ = recv_hs(&mut b);
            let mut rounds = 0u8;
            loop {
                let (_, msg) = Spake2::<Ed25519Group>::start_b(&Password::new(b"palpite"), &Identity::new(ID_A), &Identity::new(ID_B));
                if send_hs(&mut b, &Hs::PakeB { attempt: rounds, msg }).is_err() || recv_hs(&mut b).is_err() || recv_hs(&mut b).is_err() {
                    return rounds;
                }
                rounds += 1;
                let _ = send_hs(&mut b, &Hs::Retry { attempt: rounds - 1 });
            }
        });
        assert!(matches!(viewer_handshake(&mut a, "senha-certa"), Err(Error::Auth)));
        drop(a);
        assert_eq!(fake_host.join().unwrap(), MAX_ROUNDS);
    }

    #[test]
    fn giving_up_after_a_wrong_password_counts_as_wrong() {
        let (a, mut b) = socket_pair();
        let host = std::thread::spawn(move || {
            let mut a = a;
            host_handshake(&mut a, &["certa".into(), "outra".into()])
        });
        send_hs(&mut b, &Hs::Start { mode: AuthMode::Password, version: crate::PROTOCOL_VERSION }).unwrap();
        let Hs::PakeB { msg: mb, .. } = recv_hs(&mut b).unwrap() else { panic!() };
        let (st, ma) = Spake2::<Ed25519Group>::start_a(&Password::new(b"errada"), &Identity::new(ID_A), &Identity::new(ID_B));
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
