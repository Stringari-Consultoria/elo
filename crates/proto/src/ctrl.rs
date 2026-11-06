//! Mensagens entre o app e o servidor. O servidor só vê metadados (IDs,
//! nomes dos computadores, horários e volume); o conteúdo das sessões é
//! cifrado ponta a ponta.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToServer {
    /// Registro do dispositivo. `token` é um segredo aleatório do próprio
    /// dispositivo; o servidor guarda só o hash e devolve sempre o mesmo ID.
    /// `mac`: endereço da placa de rede ("aa:bb:cc:dd:ee:ff", ou vazio), para o
    /// Wake-on-LAN.
    Hello { token: [u8; 32], name: String, os: String, version: u32, mac: String },
    /// Pede conexão com outro ID.
    Connect { target: String, from_name: String },
    /// Entra numa sessão de relay (nova conexão TCP).
    Join { session: u64, secret: [u8; 16], host: bool },
    /// O anfitrião recusou o convite.
    Decline { session: u64 },
    Ping,
    /// Pede para ligar um computador desligado (Wake-on-LAN).
    Wake { target: String },
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
    /// O servidor pede a este computador que mande o pacote mágico na rede
    /// dele (está na mesma rede do computador desligado).
    WakeRequest { mac: String },
    /// Resposta ao `Wake`: quantos computadores da rede do alvo mandaram o pacote.
    WakeSent { target: String, relays: u32 },
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

/// Pacote mágico do Wake-on-LAN: 6 × 0xFF e 16 × o MAC.
pub fn magic_packet(mac: &str) -> Option<[u8; 102]> {
    let hex: Vec<u8> = mac.split([':', '-']).filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
    let mac: [u8; 6] = hex.try_into().ok()?;
    if mac == [0; 6] {
        return None;
    }
    let mut p = [0xFFu8; 102];
    for i in 0..16 {
        p[6 + i * 6..12 + i * 6].copy_from_slice(&mac);
    }
    Some(p)
}

/// Manda o pacote mágico em broadcast na rede local (portas 9 e 7).
pub fn wake_on_lan(mac: &str) -> std::io::Result<()> {
    let p = magic_packet(mac).ok_or_else(|| std::io::Error::other("endereço da placa de rede inválido"))?;
    let s = std::net::UdpSocket::bind("0.0.0.0:0")?;
    s.set_broadcast(true)?;
    for port in [9, 7] {
        s.send_to(&p, ("255.255.255.255", port))?;
    }
    Ok(())
}

/// ID no formato do usuário: "123 456 789".
pub fn pretty_id(id: &str) -> String {
    let d: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    d.as_bytes().chunks(3).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
}

pub fn clean_id(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn magic_packet() {
        let p = super::magic_packet("AA:bb:cc:01:02:03").unwrap();
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[6..12], &[0xAA, 0xBB, 0xCC, 1, 2, 3]);
        assert_eq!(&p[96..102], &[0xAA, 0xBB, 0xCC, 1, 2, 3]);
        assert!(super::magic_packet("aa-bb-cc-01-02-03").is_some());
        assert!(super::magic_packet("").is_none());
        assert!(super::magic_packet("00:00:00:00:00:00").is_none());
        assert!(super::magic_packet("aa:bb").is_none());
    }
}
