//! Preferências (JSON na pasta de configuração do usuário).

use std::path::PathBuf;

use elo_core::net::ServerCfg;
use elo_proto::msg::Permissions;
use serde::{Deserialize, Serialize};

/// Computador salvo, com um nome escolhido por quem salvou.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    pub id: String,
    /// Nome dado aqui (vazio = usa o nome do computador).
    pub alias: String,
    /// Nome que o próprio computador informa.
    pub name: String,
    pub os: String,
    /// Senha fixa (acesso não assistido) do outro computador. Vazio = pedir
    /// permissão na tela. Fica neste arquivo, que só o usuário lê (0600).
    #[serde(default)]
    pub password: String,
}

impl Favorite {
    pub fn label(&self) -> String {
        if !self.alias.trim().is_empty() {
            self.alias.trim().to_string()
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            elo_proto::ctrl::pretty_id(&self.id)
        }
    }
}

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
    /// Dias que o histórico de sessões fica guardado (LGPD); 0 = para sempre.
    pub retention_days: u32,
}

impl Default for LocalServer {
    fn default() -> Self {
        Self { enabled: true, port: elo_proto::DEFAULT_PORT, lan: true, public: String::new(), retention_days: 90 }
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
    pub favorites: Vec<Favorite>,
    /// 0 = velocidade, 1 = equilibrado, 2 = qualidade
    pub quality: usize,
    /// Ao conectar sem senha, pedir aceite (sempre verdadeiro; mantido para o futuro).
    pub allow_requests: bool,
    /// Já passou pelas boas-vindas (escolha do servidor na primeira abertura).
    pub onboarded: bool,
    /// Baixar e instalar atualizações sozinho (quando não há sessão).
    pub auto_update: bool,
    /// Fechar a janela deixa o Elo rodando na bandeja.
    pub close_to_tray: bool, /// Idioma da interface: "auto" (o do sistema), "pt" ou "en".
    pub language: String,
    /// Conta de técnico neste servidor (painel de clientes).
    pub tech: Option<TechCfg>,
    /// Elo Cliente: empresa a que este computador pertence (código de convite).
    pub company: Option<CompanyCfg>,
}

/// Entrada de técnico guardada: só o token, nunca a senha.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechCfg {
    pub user: String,
    pub name: String,
    pub admin: bool,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanyCfg {
    /// Código de convite completo ("ELO1-…").
    pub invite: String,
    pub name: String,
}

impl CompanyCfg {
    pub fn decoded(&self) -> Option<elo_proto::ctrl::Invite> {
        elo_proto::ctrl::Invite::decode(&self.invite)
    }
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
            favorites: Vec::new(),
            quality: 1,
            allow_requests: true,
            onboarded: false,
            auto_update: true,
            close_to_tray: true, language: "auto".into(),
            tech: None,
            company: None,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ELO_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    // O Elo Cliente tem configuração própria (os dois podem estar na mesma máquina).
    let name = crate::variant::app_name();
    directories::ProjectDirs::from("br.com", "Stringari", name).map(|d| d.config_dir().to_path_buf()).unwrap_or_else(|| std::env::temp_dir().join(name.to_lowercase().replace(' ', "-")))
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
        let mut s = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default();
        // Padrão antigo, de um servidor que não existe: vira "sem servidor".
        if s.server.address.trim() == "elo.stringari.com.br:21116" {
            s.server.address.clear();
        }
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

    pub fn favorite(&self, id: &str) -> Option<&Favorite> {
        self.favorites.iter().find(|f| f.id == id)
    }

    /// Salva ou tira dos favoritos. Devolve se ficou salvo.
    pub fn toggle_favorite(&mut self, id: &str, name: &str, os: &str, password: &str) -> bool {
        if self.favorite(id).is_some() {
            self.favorites.retain(|f| f.id != id);
            false
        } else {
            self.favorites.push(Favorite { id: id.into(), alias: String::new(), name: name.into(), os: os.into(), password: password.into() });
            true
        }
    }

    /// Senha salva no favorito, se houver.
    pub fn saved_password(&self, id: &str) -> Option<&str> {
        self.favorite(id).map(|f| f.password.as_str()).filter(|p| !p.is_empty())
    }

    /// Atualiza a senha de um favorito que acabou de conectar com ela.
    /// Devolve se mudou algo.
    pub fn learn_password(&mut self, id: &str, password: &str) -> bool {
        match self.favorites.iter_mut().find(|f| f.id == id) {
            Some(f) if !password.is_empty() && f.password != password => {
                f.password = password.into();
                true
            }
            _ => false,
        }
    }

    pub fn remember(&mut self, id: &str, name: &str, os: &str) {
        // Mantém o nome e o sistema dos favoritos atualizados.
        if let Some(f) = self.favorites.iter_mut().find(|f| f.id == id) {
            if !name.is_empty() {
                f.name = name.into();
            }
            if !os.is_empty() {
                f.os = os.into();
            }
        }
        self.recent.retain(|r| r.id != id);
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.recent.insert(0, Recent { id: id.into(), name: name.into(), os: os.into(), when });
        self.recent.truncate(12);
    }
}
