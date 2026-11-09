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
    /// Chave pública dos técnicos (hex; vazio = ainda não criada).
    #[serde(default)]
    pub ca: String,
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
    /// Último uso de cada token (segundos desde 1970): sem uso por
    /// `TOKEN_DAYS` dias, a entrada expira.
    #[serde(default)]
    pub token_seen: HashMap<String, u64>,
    /// Chaves dos apps do técnico e os certificados aprovados.
    #[serde(default)]
    pub keys: Vec<KeyRec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyRec {
    /// Chave pública (hex).
    pub key: String,
    /// Certificado em hex (vazio = aguardando aprovação).
    #[serde(default)]
    pub cert: String,
    /// Último uso (segundos desde 1970).
    #[serde(default)]
    pub seen: u64,
}

/// Chaves guardadas por técnico (computadores em que ele entrou).
const MAX_KEYS: usize = 10;

impl TechRec {
    /// O app do técnico entrou com esta chave: guarda (as mais antigas saem).
    pub fn note_key(&mut self, key: &str, now: u64) {
        if elo_proto::cert::key_from_hex(key).is_none() {
            return;
        }
        match self.keys.iter_mut().find(|k| k.key == key) {
            Some(k) => k.seen = now,
            None => self.keys.push(KeyRec { key: key.to_string(), cert: String::new(), seen: now }),
        }
        if self.keys.len() > MAX_KEYS {
            self.keys.sort_by_key(|k| std::cmp::Reverse(k.seen));
            self.keys.truncate(MAX_KEYS);
        }
    }
}

/// Dias sem uso que uma entrada de técnico vale.
pub const TOKEN_DAYS: u64 = 30;

impl TechRec {
    /// O token (já em hash) ainda vale?
    pub fn token_ok(&self, h: &str, now: u64) -> bool {
        self.tokens.iter().any(|t| t == h) && self.token_seen.get(h).is_some_and(|s| now.saturating_sub(*s) <= TOKEN_DAYS * 86_400)
    }

    /// Esquece as entradas vencidas.
    pub fn prune_tokens(&mut self, now: u64) {
        let seen = &self.token_seen;
        self.tokens.retain(|t| seen.get(t).is_some_and(|s| now.saturating_sub(*s) <= TOKEN_DAYS * 86_400));
        let tokens = &self.tokens;
        self.token_seen.retain(|t, _| tokens.contains(t));
    }
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
    /// Arquivo ilegível é erro, não lista vazia: antes, um `gestao.json`
    /// corrompido reabria o primeiro acesso e era sobrescrito.
    pub fn load(data: &Path) -> std::io::Result<Gestao> {
        let mut g: Gestao = load_json(&file(data))?;
        // Entradas de antes da validade: contam a partir de agora.
        let now = crate::now_secs();
        for t in g.techs.iter_mut() {
            for h in t.tokens.clone() {
                t.token_seen.entry(h).or_insert(now);
            }
        }
        Ok(g)
    }

    pub fn save(&self, data: &Path) {
        if let Ok(j) = serde_json::to_vec_pretty(self) {
            write_private(&file(data), &j);
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

    /// Técnico dono do token, se ele ainda vale. Renova a validade.
    pub fn by_token(&mut self, token: &str) -> Option<&TechRec> {
        let h = token_hash(token);
        let now = crate::now_secs();
        let t = self.techs.iter_mut().find(|t| t.token_ok(&h, now))?;
        t.token_seen.insert(h, now);
        Some(t)
    }
}

/// JSON de `p`: ausente = valor padrão; presente e ilegível = erro.
pub fn load_json<T: Default + serde::de::DeserializeOwned>(p: &Path) -> std::io::Result<T> {
    match std::fs::read(p) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| std::io::Error::other(format!("{} está ilegível ({e}). Corrija o arquivo ou renomeie-o para começar do zero; o servidor não sobe para não apagar os dados.", p.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(std::io::Error::new(e.kind(), format!("{}: {e}", p.display()))),
    }
}

/// Grava só para o dono (0600), inteiro ou nada: temporário, `fsync` e troca.
pub fn write_private(p: &Path, data: &[u8]) -> bool {
    use std::io::Write;
    // Nome próprio por gravação: duas ao mesmo tempo não se misturam.
    let mut tmp = p.as_os_str().to_os_string();
    tmp.push(format!(".tmp-{:x}", rand::thread_rng().r#gen::<u64>()));
    let tmp = PathBuf::from(tmp);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let ok = o.open(&tmp).and_then(|mut f| {
        f.write_all(data)?;
        f.sync_all()
    });
    if ok.is_err() || std::fs::rename(&tmp, p).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
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
    write_private(&p, text.as_bytes());
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
    fn unreadable_file_is_an_error_and_is_kept() {
        let d = std::env::temp_dir().join(format!("elo-gestao-{}", rand::thread_rng().r#gen::<u64>()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(Gestao::load(&d).unwrap().techs.is_empty(), "sem arquivo: começa vazio");
        std::fs::write(file(&d), b"{ quebrado").unwrap();
        assert!(Gestao::load(&d).is_err());
        assert_eq!(std::fs::read(file(&d)).unwrap(), b"{ quebrado");
        let g = Gestao { techs: vec![TechRec { user: "a".into(), name: "A".into(), hash: "h".into(), admin: true, tokens: Vec::new(), token_seen: HashMap::new(), keys: Vec::new() }], ..Default::default() };
        g.save(&d);
        assert_eq!(Gestao::load(&d).unwrap().techs.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(file(&d)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unused_tech_entry_expires_and_use_renews_it() {
        let h = token_hash("tok");
        let now = crate::now_secs();
        let mut g = Gestao { techs: vec![TechRec { user: "a".into(), name: "A".into(), hash: String::new(), admin: false, tokens: vec![h.clone()], token_seen: HashMap::from([(h.clone(), now - (TOKEN_DAYS + 1) * 86_400)]), keys: Vec::new() }], ..Default::default() };
        assert!(g.by_token("tok").is_none(), "vencida");
        g.techs[0].token_seen.insert(h.clone(), now - (TOKEN_DAYS - 1) * 86_400);
        assert!(g.by_token("tok").is_some());
        assert!(g.techs[0].token_seen[&h] >= now, "o uso renova");
        g.techs[0].token_seen.insert(h.clone(), 0);
        g.techs[0].prune_tokens(now);
        assert!(g.techs[0].tokens.is_empty());
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
