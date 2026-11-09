//! Chave dos técnicos: quem atende um computador de empresa (Elo Cliente)
//! prova quem é de ponta a ponta, sem depender do servidor.
//!
//! - O administrador cria a chave dos técnicos (Ed25519) no app dele; só a
//!   parte pública vai para o servidor e para os convites.
//! - Cada técnico tem a própria chave, criada no app dele. O administrador
//!   aprova a chave (conferindo o código com o técnico) e o app dele assina um
//!   certificado: usuário, nome, chave do técnico e validade.
//! - O Elo Cliente guarda a chave pública dos técnicos que veio no convite.
//!   Num pedido de acesso, o técnico mostra o certificado e assina o vínculo
//!   da sessão (derivado da chave do canal): o servidor não consegue se passar
//!   por técnico nem ficar no meio sem que a assinatura falhe.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Validade de um certificado. O app do administrador renova sozinho os
/// técnicos que continuam na lista; quem foi removido para de valer em até
/// este prazo, mesmo guardando o certificado.
pub const CERT_DAYS: u64 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechCert {
    pub user: String,
    /// Nome que o cliente vê no pedido de acesso.
    pub name: String,
    /// Chave pública do técnico.
    pub key: [u8; 32],
    /// Vale até (segundos desde 1970).
    pub expires: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedCert {
    pub cert: TechCert,
    pub sig: Vec<u8>,
}

/// Chave secreta nova (técnico ou chave dos técnicos).
pub fn new_secret() -> [u8; 32] {
    rand::random()
}

pub fn public_of(secret: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret).verifying_key().to_bytes()
}

fn cert_message(c: &TechCert) -> Vec<u8> {
    let mut m = b"elo-certificado-tecnico-v1".to_vec();
    m.extend(postcard::to_stdvec(c).unwrap_or_default());
    m
}

fn session_message(binding: &[u8; 32]) -> Vec<u8> {
    let mut m = b"elo-sessao-tecnico-v1".to_vec();
    m.extend_from_slice(binding);
    m
}

fn verify(key: &[u8; 32], msg: &[u8], sig: &[u8]) -> bool {
    let (Ok(k), Ok(s)) = (VerifyingKey::from_bytes(key), Signature::from_slice(sig)) else { return false };
    k.verify(msg, &s).is_ok()
}

/// Certificado assinado com a chave dos técnicos.
pub fn issue(ca_secret: &[u8; 32], cert: TechCert) -> SignedCert {
    let sig = SigningKey::from_bytes(ca_secret).sign(&cert_message(&cert)).to_bytes().to_vec();
    SignedCert { cert, sig }
}

impl SignedCert {
    /// Assinado por `ca` e dentro da validade.
    pub fn valid(&self, ca: &[u8; 32], now: u64) -> bool {
        now <= self.cert.expires && verify(ca, &cert_message(&self.cert), &self.sig)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(self).unwrap_or_default()
    }

    pub fn from_bytes(b: &[u8]) -> Option<SignedCert> {
        postcard::from_bytes(b).ok()
    }

    pub fn to_text(&self) -> String {
        to_hex(&self.to_bytes())
    }

    pub fn from_text(s: &str) -> Option<SignedCert> {
        Self::from_bytes(&from_hex(s)?)
    }
}

/// Prova do técnico numa sessão: assina o vínculo do canal.
pub fn prove(tech_secret: &[u8; 32], binding: &[u8; 32]) -> Vec<u8> {
    SigningKey::from_bytes(tech_secret).sign(&session_message(binding)).to_bytes().to_vec()
}

/// Confere a prova de um técnico: certificado válido pela chave `ca` e
/// assinatura do vínculo desta sessão com a chave do certificado.
pub fn check(ca: &[u8; 32], now: u64, cert: &[u8], sig: &[u8], binding: &[u8; 32]) -> Option<TechCert> {
    let c = SignedCert::from_bytes(cert)?;
    (c.valid(ca, now) && verify(&c.cert.key, &session_message(binding), sig)).then_some(c.cert)
}

/// Código curto de uma chave para conferir de olho ("3F2A-91C0-…").
pub fn fingerprint(key: &[u8; 32]) -> String {
    let h = Sha256::digest(key);
    let hex = to_hex(&h[..10]).to_uppercase();
    hex.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join("-")
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// 32 bytes em hexadecimal (chave).
pub fn key_from_hex(s: &str) -> Option<[u8; 32]> {
    from_hex(s)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_tech_proves_and_others_do_not() {
        let ca = new_secret();
        let tech = new_secret();
        let binding = [7u8; 32];
        let c = issue(&ca, TechCert { user: "ana".into(), name: "Ana".into(), key: public_of(&tech), expires: 1000 });
        let ca_pub = public_of(&ca);
        let sig = prove(&tech, &binding);
        assert_eq!(check(&ca_pub, 999, &c.to_bytes(), &sig, &binding).map(|c| c.name), Some("Ana".into()));
        // Vencido, outra sessão, outra chave dos técnicos, nome trocado.
        assert!(check(&ca_pub, 1001, &c.to_bytes(), &sig, &binding).is_none());
        assert!(check(&ca_pub, 999, &c.to_bytes(), &sig, &[8u8; 32]).is_none());
        assert!(check(&public_of(&new_secret()), 999, &c.to_bytes(), &sig, &binding).is_none());
        let mut forged = c.clone();
        forged.cert.name = "Carlos".into();
        assert!(check(&ca_pub, 999, &forged.to_bytes(), &sig, &binding).is_none());
        // Certificado de outro técnico com a assinatura deste não vale.
        assert!(check(&ca_pub, 999, &c.to_bytes(), &prove(&new_secret(), &binding), &binding).is_none());
        assert_eq!(SignedCert::from_text(&c.to_text()), Some(c));
        assert_eq!(key_from_hex(&to_hex(&ca_pub)), Some(ca_pub));
        assert_eq!(fingerprint(&ca_pub).len(), 24);
    }
}
