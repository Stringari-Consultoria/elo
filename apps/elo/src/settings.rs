//! Preferências (JSON na pasta de configuração do usuário).

use std::io::Write;
use std::path::{Path, PathBuf};

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
    pub close_to_tray: bool,
    /// Idioma da interface: "auto" (o do sistema), "pt" ou "en".
    pub language: String,
    /// Conta de técnico neste servidor (painel de clientes).
    pub tech: Option<TechCfg>,
    /// Elo Cliente: empresa a que este computador pertence (código de convite).
    pub company: Option<CompanyCfg>,
    /// Chave secreta deste app como técnico (hex). Um administrador aprova a
    /// parte pública; com ela o técnico prova quem é aos computadores das empresas.
    pub tech_secret: String,
    /// Administrador: chave secreta dos técnicos (hex), se foi criada ou
    /// importada aqui. Quem a tem aprova técnicos e gera convites.
    pub tech_ca_secret: String,
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
    /// Código de convite completo ("ELO2-…").
    pub invite: String,
    pub name: String,
    /// Chave pública dos técnicos da empresa (hex), do convite ou, nos
    /// convites antigos, do primeiro contato com o servidor. Só técnico
    /// aprovado por ela consegue pedir acesso.
    #[serde(default)]
    pub ca: String,
}

impl CompanyCfg {
    pub fn decoded(&self) -> Option<elo_proto::ctrl::Invite> {
        elo_proto::ctrl::Invite::decode(&self.invite)
    }

    pub fn ca_key(&self) -> Option<[u8; 32]> {
        elo_proto::cert::key_from_hex(&self.ca)
    }
}

impl Settings {
    /// Chave secreta deste app de técnico.
    pub fn tech_key(&self) -> Option<[u8; 32]> {
        elo_proto::cert::key_from_hex(&self.tech_secret)
    }

    /// Chave secreta dos técnicos, se está neste computador.
    pub fn ca_secret(&self) -> Option<[u8; 32]> {
        elo_proto::cert::key_from_hex(&self.tech_ca_secret)
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
            tech_secret: String::new(),
            tech_ca_secret: String::new(),
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

/// Lê as configurações. Arquivo que existe mas não dá para usar é guardado ao
/// lado (`.bad-<hora>`), em vez de ser sobrescrito: nele estão o token do
/// dispositivo (o ID) e os favoritos.
fn read_or_set_aside(p: &Path) -> Option<Settings> {
    let why = match std::fs::read(p) {
        Ok(b) => match serde_json::from_slice(&b) {
            Ok(s) => return Some(s),
            Err(e) => e.to_string(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => e.to_string(),
    };
    let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let bad = p.with_extension(format!("json.bad-{when}"));
    let _ = std::fs::rename(p, &bad);
    eprintln!("configurações ilegíveis ({why}): guardadas em {}", bad.display());
    None
}

/// Grava de uma vez: num temporário só do usuário (guarda o segredo do
/// dispositivo e a senha fixa) e depois renomeia. Queda de energia no meio não
/// estraga o arquivo que já existia.
fn write_private(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = p.with_extension(format!("json.tmp-{}", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let written = o.open(&tmp).and_then(|mut f| {
        f.write_all(data)?;
        f.sync_all()
    });
    let result = written.and_then(|_| std::fs::rename(&tmp, p));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl Settings {
    pub fn load() -> Self {
        let p = config_dir().join("elo.json");
        // Sempre regrava: campos novos (inclusive o token do dispositivo, que
        // define o ID) ficam persistidos já na primeira abertura.
        let mut s = read_or_set_aside(&p).unwrap_or_default();
        // Padrão antigo, de um servidor que não existe: vira "sem servidor".
        if s.server.address.trim() == "elo.stringari.com.br:21116" {
            s.server.address.clear();
        }
        if elo_proto::cert::key_from_hex(&s.tech_secret).is_none() {
            s.tech_secret = elo_proto::cert::to_hex(&elo_proto::cert::new_secret());
        }
        s.save();
        s
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("elo.json");
        if let Ok(b) = serde_json::to_vec_pretty(self)
            && let Err(e) = write_private(&p, &b)
        {
            eprintln!("não foi possível salvar {}: {e}", p.display());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("elo-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unreadable_settings_are_kept_aside_not_overwritten() {
        let d = temp_dir("ruim");
        let p = d.join("elo.json");
        std::fs::write(&p, b"{ truncado").unwrap();
        assert!(read_or_set_aside(&p).is_none());
        assert!(!p.exists());
        let bad: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(bad.len(), 1);
        assert_eq!(std::fs::read(bad[0].path()).unwrap(), b"{ truncado");
    }

    #[test]
    fn missing_settings_are_not_an_error() {
        let d = temp_dir("novo");
        assert!(read_or_set_aside(&d.join("elo.json")).is_none());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn save_replaces_the_file_and_leaves_no_temporary() {
        let d = temp_dir("grava");
        let p = d.join("elo.json");
        write_private(&p, b"um").unwrap();
        write_private(&p, b"dois").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"dois");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
