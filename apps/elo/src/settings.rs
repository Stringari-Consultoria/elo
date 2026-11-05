//! Preferências (JSON na pasta de configuração do usuário).

use std::path::PathBuf;

use elo_core::net::ServerCfg;
use elo_proto::msg::Permissions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recent {
    pub id: String,
    pub name: String,
    pub os: String,
    pub when: u64,
}

/// Servidor embutido: sobe junto com o app e os outros computadores conectam
/// nele (IP deste computador + porta + impressão digital).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalServer {
    pub enabled: bool,
    pub port: u16,
    /// Aceitar conexões diretas da rede (falso = só deste computador, por
    /// exemplo atrás do Cloudflare Tunnel).
    pub lan: bool,
    /// Endereço pelo qual os outros chegam aqui (ex.: "wss://elo.dominio.com"
    /// do Cloudflare Tunnel). Vazio = IP da rede local.
    pub public: String,
}

impl Default for LocalServer {
    fn default() -> Self {
        Self { enabled: true, port: elo_proto::DEFAULT_PORT, lan: true, public: String::new() }
    }
}

impl LocalServer {
    /// Mudanças que exigem reiniciar o servidor.
    pub fn same_bind(&self, o: &LocalServer) -> bool {
        (self.enabled, self.port, self.lan) == (o.enabled, o.port, o.lan)
    }

    /// Endereço público via WebSocket (dispensa a impressão digital).
    pub fn public_ws(&self) -> Option<&str> {
        let p = self.public.trim();
        (p.starts_with("wss://") || p.starts_with("ws://")).then_some(p)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Segredo do dispositivo (o servidor guarda só o hash e devolve o mesmo ID).
    pub token: [u8; 32],
    /// Servidor local ligado: conecta nele e ignora `server`.
    pub local_server: LocalServer,
    /// Servidor externo (usado quando o local está desligado).
    pub server: ServerCfg,
    pub device_name: String,
    pub dark: bool,
    /// Acesso não assistido (senha fixa).
    pub unattended: bool,
    pub permanent_password: String,
    pub default_permissions: Permissions,
    pub downloads: PathBuf,
    pub recent: Vec<Recent>,
    /// 0 = velocidade, 1 = equilibrado, 2 = qualidade
    pub quality: usize,
    /// Ao conectar sem senha, pedir aceite (sempre verdadeiro; mantido para o futuro).
    pub allow_requests: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let downloads = directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.join("Elo"))).unwrap_or_else(|| std::env::temp_dir().join("Elo"));
        Self {
            token: rand::random(),
            local_server: LocalServer::default(),
            server: ServerCfg::default(),
            device_name: elo_core::platform::computer_name(),
            dark: false,
            unattended: false,
            permanent_password: String::new(),
            default_permissions: Permissions::default(),
            downloads,
            recent: Vec::new(),
            quality: 1,
            allow_requests: true,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ELO_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    directories::ProjectDirs::from("br.com", "Stringari", "Elo").map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| std::env::temp_dir().join("elo"))
}

/// Dados do servidor local (IDs, sessões, certificado autoassinado).
pub fn server_data_dir() -> PathBuf {
    config_dir().join("servidor")
}

impl Settings {
    pub fn load() -> Self {
        let p = config_dir().join("elo.json");
        // Sempre regrava: campos novos (inclusive o token do dispositivo, que
        // define o ID) ficam persistidos já na primeira abertura.
        let s = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default();
        s.save();
        s
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("elo.json");
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(&p, b);
            // O arquivo guarda o segredo do dispositivo e a senha fixa: só o usuário lê.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    pub fn remember(&mut self, id: &str, name: &str, os: &str) {
        self.recent.retain(|r| r.id != id);
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.recent.insert(0, Recent { id: id.into(), name: name.into(), os: os.into(), when });
        self.recent.truncate(12);
    }
}
