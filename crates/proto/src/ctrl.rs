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
    // Daqui para baixo: só para servidores que anunciam "empresas" (ver
    // `SERVER_FEATURES`). Variantes novas entram sempre no fim.
    /// Elo Cliente: liga este computador à empresa do código de convite.
    JoinCompany { company: String, key: String },
    /// Técnico: entra com usuário e senha. Com `setup` (código de primeiro
    /// acesso do servidor), cria a conta como administrador.
    /// `key`: chave pública do técnico neste app (hex), para a aprovação.
    Login { user: String, password: String, setup: Option<String>, name: String, key: String },
    /// Técnico: volta a entrar com o token de uma entrada anterior.
    Resume { token: String, key: String },
    Logout,
    /// Elo Cliente: pede ajuda aos técnicos.
    RequestHelp { message: String },
    CancelHelp,
    /// Técnico: gestão de empresas, computadores, técnicos e pedidos.
    Manage(Manage),
}

/// Recursos que o servidor anuncia logo depois do `Welcome` (no mesmo
/// quadro; versões antigas do app ignoram os bytes a mais).
pub const SERVER_FEATURES: &[&str] = &["empresas"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Manage {
    /// Pede a fotografia atual (`Overview`).
    Refresh,
    CreateCompany { name: String },
    RenameCompany { id: String, name: String },
    DeleteCompany { id: String },
    /// Troca a chave do convite (os convites antigos deixam de valer para
    /// computadores novos; os já ligados continuam).
    NewKey { id: String },
    /// Empresa, apelido e observações de um computador.
    SetDevice { id: String, company: Option<String>, alias: String, notes: String },
    /// Tira o computador da lista (e da empresa).
    ForgetDevice { id: String },
    CreateTech { user: String, name: String, password: String, admin: bool },
    DeleteTech { user: String },
    SetPassword { user: String, password: String },
    /// "Vou atender": marca o pedido de ajuda como meu.
    TakeRequest { device: String },
    /// Administrador: chave pública dos técnicos (hex). A secreta fica no app
    /// de quem criou; trocar a chave invalida as aprovações e os convites.
    SetCa { public: String },
    /// Administrador: certificado do técnico (`cert::SignedCert` em hex),
    /// assinado no app de quem tem a chave dos técnicos.
    Certify { user: String, cert: String },
}

/// O que o técnico vê no painel de clientes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub me: Tech,
    pub companies: Vec<Company>,
    pub devices: Vec<DeviceEntry>,
    pub requests: Vec<HelpRequest>,
    /// Só para administradores.
    pub techs: Vec<Tech>,
    /// Chave pública dos técnicos (hex; vazio = ainda não criada).
    pub ca: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Company {
    pub id: String,
    pub name: String,
    /// Chave do convite (o app junta com o endereço do servidor).
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tech {
    pub user: String,
    pub name: String,
    pub admin: bool,
    /// Chaves dos apps em que o técnico entrou (um por computador).
    pub keys: Vec<TechKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TechKey {
    /// Chave pública (hex).
    pub key: String,
    /// Certificado (hex; vazio = aguardando aprovação).
    pub cert: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    /// Nome que o computador informa.
    pub name: String,
    pub os: String,
    /// Nome dado pelos técnicos.
    pub alias: String,
    pub notes: String,
    pub company: Option<String>,
    pub online: bool,
    /// Visto pela última vez (segundos desde 1970).
    pub last_seen: u64,
    pub ip: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HelpRequest {
    pub device: String,
    pub name: String,
    pub company: Option<String>,
    pub message: String,
    pub since: u64,
    /// Técnico que disse que vai atender.
    pub taken_by: Option<String>,
}

/// Convite do Elo Cliente: onde fica o servidor, de que empresa é e a chave
/// pública dos técnicos (só eles conseguem acessar o computador).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub address: String,
    pub tls: bool,
    pub pin: String,
    pub company: String,
    pub key: String,
    pub name: String,
    /// Chave pública dos técnicos (hex; vazio nos convites antigos).
    pub ca: String,
}

/// Convite de antes da chave dos técnicos ("ELO1-").
#[derive(Deserialize)]
struct InviteV1 {
    address: String,
    tls: bool,
    pin: String,
    company: String,
    key: String,
    name: String,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

impl Invite {
    /// Texto para copiar e colar: "ELO2-" + base64 (URL) do conteúdo.
    pub fn encode(&self) -> String {
        let data = postcard::to_stdvec(self).unwrap_or_default();
        let mut out = String::from("ELO2-");
        for c in data.chunks(3) {
            let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
            for i in 0..=c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Invite> {
        let t: String = text.trim().chars().filter(|c| !c.is_whitespace()).collect();
        let (v1, body) = match (t.strip_prefix("ELO2-"), t.strip_prefix("ELO1-")) {
            (Some(b), _) => (false, b),
            (None, Some(b)) => (true, b),
            _ => return None,
        };
        let mut bytes = Vec::new();
        let vals: Vec<u32> = body.bytes().map(|b| B64.iter().position(|x| *x == b).map(|p| p as u32)).collect::<Option<_>>()?;
        for c in vals.chunks(4) {
            let n = c.iter().enumerate().fold(0u32, |n, (i, v)| n | v << (18 - 6 * i));
            for i in 0..c.len().saturating_sub(1) {
                bytes.push((n >> (16 - 8 * i)) as u8);
            }
        }
        if v1 {
            let o: InviteV1 = postcard::from_bytes(&bytes).ok()?;
            return Some(Invite { address: o.address, tls: o.tls, pin: o.pin, company: o.company, key: o.key, name: o.name, ca: String::new() });
        }
        postcard::from_bytes(&bytes).ok()
    }
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
    // Só para quem usou os recursos de "empresas".
    /// Elo Cliente: ligado à empresa. `ca`: chave pública dos técnicos (hex).
    Company { name: String, ca: String },
    /// Técnico: entrou. Guarde o `token` para `Resume`.
    LoggedIn { user: String, name: String, admin: bool, token: String },
    /// O servidor ainda não tem técnicos: crie o administrador com o código
    /// de primeiro acesso (aparece nos registros do servidor).
    NeedsSetup,
    /// Elo Cliente: situação do pedido de ajuda (`None` = na fila).
    HelpStatus { taken_by: Option<String> },
    Overview(Overview),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    NotFound,
    Offline,
    RateLimited,
    BadRequest,
    Version,
    Expired,
    /// Usuário ou senha do técnico errados (ou sem permissão).
    Auth,
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
    fn invite_round_trip() {
        let i = super::Invite { address: "wss://elo.exemplo.com".into(), tls: true, pin: String::new(), company: "c1".into(), key: "k".repeat(22), name: "Padaria Pão Quente".into(), ca: "ab".repeat(32) };
        let t = i.encode();
        assert!(t.starts_with("ELO2-"));
        // Convite antigo (sem a chave dos técnicos) continua valendo.
        let old = "ELO1-FXdzczovL2Vsby5leGVtcGxvLmNvbQEAAmMxFmtra2tra2tra2tra2tra2tra2tra2sETG9qYQ";
        let o = super::Invite::decode(old).expect("ELO1");
        assert_eq!((o.address.as_str(), o.company.as_str(), o.name.as_str(), o.ca.as_str()), ("wss://elo.exemplo.com", "c1", "Loja", ""));
        assert_eq!(super::Invite::decode(&t), Some(i.clone()));
        // Colado com espaços/quebras de linha no meio.
        let spaced: String = t.chars().enumerate().flat_map(|(n, c)| if n % 20 == 19 { vec![c, '\n'] } else { vec![c] }).collect();
        assert_eq!(super::Invite::decode(&spaced), Some(i));
        assert!(super::Invite::decode("ELO1-@@@").is_none());
        assert!(super::Invite::decode("qualquer coisa").is_none());
    }

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
