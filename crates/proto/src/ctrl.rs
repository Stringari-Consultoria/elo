//! Mensagens entre o app e o servidor. O servidor só vê metadados (IDs,
//! nomes dos computadores, horários e volume); o conteúdo das sessões é
//! cifrado ponta a ponta.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToServer {
    /// Registro do dispositivo. `token` é um segredo aleatório do próprio
    /// dispositivo; o servidor guarda só o hash e devolve sempre o mesmo ID.
    Hello { token: [u8; 32], name: String, os: String, version: u32 },
    /// Pede conexão com outro ID.
    Connect { target: String, from_name: String },
    /// Entra numa sessão de relay (nova conexão TCP).
    Join { session: u64, secret: [u8; 16], host: bool },
    /// O anfitrião recusou o convite.
    Decline { session: u64 },
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromServer {
    Welcome { id: String },
    /// Convite para o anfitrião.
    Incoming { session: u64, secret: [u8; 16], from_id: String, from_name: String },
    /// Para quem pediu: a sessão está pronta para `Join`.
    SessionReady { session: u64, secret: [u8; 16], target_name: String, target_os: String },
    /// Os dois lados entraram; daqui em diante o servidor só repassa bytes.
    Paired,
    Declined { session: u64 },
    Error { code: ErrorCode, message: String },
    Pong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    NotFound,
    Offline,
    RateLimited,
    BadRequest,
    Version,
    Expired,
}

/// ID no formato do usuário: "123 456 789".
pub fn pretty_id(id: &str) -> String {
    let d: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    d.as_bytes().chunks(3).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
}

pub fn clean_id(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}
