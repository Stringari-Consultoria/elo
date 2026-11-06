//! Empresas, técnicos e computadores das empresas (`gestao.json`).
//!
//! - Técnicos entram com usuário e senha (Argon2); o app guarda só um token.
//! - O primeiro administrador é criado com o código de primeiro acesso, que
//!   o servidor gera enquanto não houver técnicos (aparece nos registros e em
//!   `primeiro-acesso.txt`).
//! - Computadores ligados a uma empresa (Elo Cliente) só recebem conexões de
//!   técnicos logados e não conectam em ninguém.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Gestao {
    #[serde(default)]
    pub companies: Vec<CompanyRec>,
    #[serde(default)]
    pub techs: Vec<TechRec>,
    /// ID → empresa, apelido e observações.
    #[serde(default)]
    pub devices: HashMap<String, DevRec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyRec {
    pub id: String,
    pub name: String,
    pub key: String,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechRec {
    pub user: String,
    pub name: String,
    /// Hash Argon2 (formato PHC).
    pub hash: String,
    pub admin: bool,
    /// SHA-256 dos tokens das entradas ativas (um por app).
    #[serde(default)]
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DevRec {
    pub company: Option<String>,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub notes: String,
}

impl Gestao {
    pub fn load(data: &Path) -> Gestao {
        std::fs::read(file(data)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, data: &Path) {
        let Ok(j) = serde_json::to_vec_pretty(self) else { return };
        let tmp = data.join("gestao.json.tmp");
        if std::fs::write(&tmp, j).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::rename(&tmp, file(data));
        }
    }

    pub fn company_of(&self, device: &str) -> Option<String> {
        self.devices.get(device).and_then(|d| d.company.clone()).filter(|c| self.companies.iter().any(|x| x.id == *c))
    }

    pub fn company_name(&self, id: &str) -> Option<String> {
        self.companies.iter().find(|c| c.id == id).map(|c| c.name.clone())
    }

    pub fn tech(&self, user: &str) -> Option<&TechRec> {
        self.techs.iter().find(|t| t.user == user)
    }

    /// Técnico dono do token (já em hash).
    pub fn by_token(&self, token: &str) -> Option<&TechRec> {
        let h = token_hash(token);
        self.techs.iter().find(|t| t.tokens.contains(&h))
    }
}

fn file(data: &Path) -> PathBuf {
    data.join("gestao.json")
}

/// Usuário normalizado: minúsculo, sem espaços nas pontas.
pub fn norm_user(u: &str) -> String {
    u.trim().to_lowercase()
}

pub fn token_hash(t: &str) -> String {
    format!("{:x}", Sha256::digest(t.as_bytes()))
}

/// Texto aleatório com letras e números (sem os que se confundem).
pub fn random_text(n: usize) -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..n).map(|_| A[rng.gen_range(0..A.len())] as char).collect()
}

/// Código de primeiro acesso: "abcd-efgh".
pub fn setup_code() -> String {
    format!("{}-{}", random_text(4), random_text(4))
}

/// Hash Argon2 da senha (lento de propósito: chame fora do tokio).
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt: [u8; 16] = rand::thread_rng().r#gen();
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| argon2::Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// Grava o código de primeiro acesso num arquivo que só o dono lê.
pub fn write_setup_file(data: &Path, code: &str) {
    let p = data.join("primeiro-acesso.txt");
    let text = format!(
        "Código de primeiro acesso do servidor do Elo: {code}\n\nNo app do Elo, em Configurações → Conta de técnico, use este código para\ncriar a conta de administrador. Ele deixa de valer depois disso.\n"
    );
    if std::fs::write(&p, text).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
}

pub fn remove_setup_file(data: &Path) {
    let _ = std::fs::remove_file(data.join("primeiro-acesso.txt"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_round_trip() {
        let h = hash_password("s3nha-boa").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password(&h, "s3nha-boa"));
        assert!(!verify_password(&h, "outra"));
        assert!(!verify_password("lixo", "s3nha-boa"));
    }

    #[test]
    fn company_must_exist() {
        let mut g = Gestao::default();
        g.devices.insert("1".into(), DevRec { company: Some("c".into()), ..Default::default() });
        assert_eq!(g.company_of("1"), None, "empresa apagada não vale");
        g.companies.push(CompanyRec { id: "c".into(), name: "Padaria".into(), key: "k".into(), created: 0 });
        assert_eq!(g.company_of("1"), Some("c".into()));
    }
}
