//! Atualização automática.
//!
//! A cada release, o CI publica `latest.json` (versão, endereço base e, por
//! pacote, nome, tamanho e SHA-256) e `latest.json.sig`, a assinatura Ed25519
//! do manifesto com a chave privada guardada como secret do GitHub. O app:
//!
//! 1. baixa os dois e confere a assinatura com a chave pública embutida
//!    ([`PUBLIC_KEY`]); sem assinatura válida, nada é instalado;
//! 2. só aceita versão mais nova que a atual (não volta para uma antiga);
//! 3. baixa o pacote do sistema e confere o SHA-256 do manifesto;
//! 4. instala do jeito de cada sistema e reabre o Elo.
//!
//! Para testes, `ELO_UPDATE_URL` troca o endereço do manifesto.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Chave pública que assina os manifestos (a privada fica só no GitHub).
pub const PUBLIC_KEY: [u8; 32] = hex32("c3c399f071fbfd079b48b50a7464bacd5b0f47f46d8c41b7b3081f5614fc1386");

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const MANIFEST_URL: &str = "https://github.com/Stringari-Consultoria/elo/releases/latest/download/latest.json";

const fn hex32(s: &str) -> [u8; 32] {
    const fn v(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("hex inválido"),
        }
    }
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = v(b[2 * i]) << 4 | v(b[2 * i + 1]);
        i += 1;
    }
    out
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// Endereço da pasta da release (termina em `/`).
    pub base_url: String,
    pub assets: HashMap<String, Asset>,
}

/// Versão `a` é mais nova que `b` (números separados por ponto).
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| s.trim_start_matches('v').split(['.', '-']).take(3).map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parse(a) > parse(b)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(15))).timeout_recv_response(Some(Duration::from_secs(30))).user_agent(format!("Elo/{CURRENT}")).build().into()
}

fn get_small(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut r = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    r.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{url}: {e}"))
}

/// Confere a assinatura Ed25519 do manifesto.
pub fn verify(msg: &[u8], sig: &[u8], key: &[u8; 32]) -> Result<(), String> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let key = VerifyingKey::from_bytes(key).map_err(|_| "chave de atualização inválida".to_string())?;
    let sig: [u8; 64] = sig.try_into().map_err(|_| "assinatura do manifesto com tamanho errado".to_string())?;
    key.verify_strict(msg, &Signature::from_bytes(&sig)).map_err(|_| "assinatura do manifesto não confere: atualização recusada".to_string())
}

/// Baixa e valida o manifesto em `url` com a chave `key`.
pub fn fetch_manifest(url: &str, key: &[u8; 32]) -> Result<Manifest, String> {
    let body = get_small(url, 256 * 1024)?;
    let sig = get_small(&format!("{url}.sig"), 1024)?;
    verify(&body, &sig, key)?;
    serde_json::from_slice(&body).map_err(|e| format!("manifesto inválido: {e}"))
}

/// Como este Elo foi instalado (decide qual pacote baixar e como trocar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Instalador do Windows (Inno Setup).
    WindowsSetup { all_users: bool },
    /// Elo.app no macOS.
    MacApp(PathBuf),
    /// Executáveis numa pasta do usuário (pacote .tar.gz).
    LinuxDir(PathBuf),
    /// Pacote .deb (em /usr: precisa de administrador).
    LinuxDeb,
    /// Não dá para atualizar sozinho (portátil, desenvolvimento): só avisa.
    Manual(String),
}

impl Install {
    pub fn asset_key(&self) -> Option<&'static str> {
        match self {
            Install::WindowsSetup { .. } => Some("windows-x64"),
            Install::MacApp(_) => Some("macos"),
            Install::LinuxDir(_) => Some("linux-x86_64"),
            Install::LinuxDeb => Some("linux-x86_64-deb"),
            Install::Manual(_) => None,
        }
    }
}

pub fn detect() -> Install {
    if cfg!(debug_assertions) {
        return Install::Manual("versão de desenvolvimento: atualize pelo código".into());
    }
    let Ok(exe) = std::env::current_exe() else { return Install::Manual("caminho do Elo desconhecido".into()) };
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    if cfg!(target_os = "windows") {
        // O desinstalador do Inno Setup fica ao lado do elo.exe; sem ele, é o .zip portátil.
        if dir.join("unins000.exe").exists() {
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            let all_users = !local.is_some_and(|l| dir.starts_with(l));
            return Install::WindowsSetup { all_users };
        }
        return Install::Manual("versão portátil: baixe a nova no GitHub".into());
    }
    if cfg!(target_os = "macos") {
        return match exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
            Some(app) if !app.starts_with("/Volumes") => Install::MacApp(app.to_path_buf()),
            Some(_) => Install::Manual("arraste o Elo para Aplicativos para receber atualizações".into()),
            None => Install::Manual("fora do Elo.app: atualize manualmente".into()),
        };
    }
    if dir.starts_with("/usr") {
        return Install::LinuxDeb;
    }
    // Pasta do usuário (instalar.sh): dá para trocar os arquivos sem administrador.
    let probe = dir.join(".elo-atualizacao-teste");
    if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(&probe);
        Install::LinuxDir(dir)
    } else {
        Install::Manual(format!("sem permissão para gravar em {}", dir.display()))
    }
}

/// Resultado de uma verificação.
pub enum Check {
    UpToDate,
    /// Versão nova; `None` no asset quando este jeito de instalar não se atualiza sozinho.
    Available(Manifest, Option<Asset>),
}

pub fn check(install: &Install) -> Result<Check, String> {
    let url = std::env::var("ELO_UPDATE_URL").unwrap_or_else(|_| MANIFEST_URL.to_string());
    let m = fetch_manifest(&url, &PUBLIC_KEY)?;
    if !newer(&m.version, CURRENT) {
        return Ok(Check::UpToDate);
    }
    let asset = install.asset_key().and_then(|k| m.assets.get(k).cloned());
    Ok(Check::Available(m, asset))
}

/// Pasta para os downloads de atualização.
fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("br.com", "Stringari", "Elo").map(|d| d.cache_dir().join("atualizacoes")).unwrap_or_else(|| std::env::temp_dir().join("elo-atualizacoes"))
}

/// Baixa o pacote e confere tamanho e SHA-256. `progress` recebe os bytes baixados.
pub fn download(m: &Manifest, a: &Asset, progress: &Arc<AtomicU64>) -> Result<PathBuf, String> {
    download_to(&format!("{}{}", m.base_url, a.name), a, &cache_dir(), progress)
}

pub fn download_to(url: &str, a: &Asset, dir: &Path, progress: &Arc<AtomicU64>) -> Result<PathBuf, String> {
    // Só o nome do arquivo (o manifesto é assinado, mas não custa).
    let name = Path::new(&a.name).file_name().ok_or("nome de pacote inválido")?.to_owned();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    let part = path.with_extension("parte");
    let r = agent().get(url).call().map_err(|e| format!("baixando {}: {e}", a.name))?;
    let mut body = r.into_body().into_reader();
    let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("baixando {}: {e}", a.name))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > a.size {
            return Err("o pacote veio maior que o anunciado: atualização recusada".into());
        }
        hash.update(&buf[..n]);
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress.store(total, Ordering::Relaxed);
    }
    drop(f);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if total != a.size || !got.eq_ignore_ascii_case(&a.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err("o pacote baixado não confere com o manifesto assinado: atualização recusada".into());
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{:?} falhou: {}", cmd.get_program(), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Aspas simples para `sh -c`.
#[cfg(unix)]
fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// Reabre o Elo depois que este processo sair (a porta do servidor fica livre).
#[cfg(unix)]
fn relaunch(cmd: &str) -> Result<(), String> {
    Command::new("/bin/sh").arg("-c").arg(format!("sleep 2; {cmd}")).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Instala o pacote baixado. Em caso de sucesso, quem chamou deve encerrar o
/// app logo em seguida (o Elo novo abre sozinho).
pub fn install(how: &Install, file: &Path, background: bool) -> Result<(), String> {
    let _ = background;
    match how {
        Install::Manual(why) => Err(why.clone()),
        #[cfg(target_os = "windows")]
        Install::WindowsSetup { all_users } => {
            // O instalador fecha o Elo, instala em silêncio e reabre (seção [Run]).
            Command::new(file)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", if *all_users { "/ALLUSERS" } else { "/CURRENTUSER" }])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("instalador: {e}"))
        }
        #[cfg(target_os = "macos")]
        Install::MacApp(app) => {
            let mnt = std::env::temp_dir().join(format!("elo-atualizacao-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&mnt);
            run(Command::new("hdiutil").arg("attach").arg("-nobrowse").arg("-readonly").arg("-noautoopen").arg("-mountpoint").arg(&mnt).arg(file))?;
            let staging = app.with_extension("app.novo");
            let _ = std::fs::remove_dir_all(&staging);
            let copied = run(Command::new("ditto").arg(mnt.join("Elo.app")).arg(&staging));
            let _ = run(Command::new("hdiutil").arg("detach").arg("-force").arg(&mnt));
            copied?;
            // Troca: o processo atual continua com os arquivos antigos até sair.
            let old = app.with_extension("app.antigo");
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(app, &old).map_err(|e| format!("{}: {e}", app.display()))?;
            if let Err(e) = std::fs::rename(&staging, app) {
                let _ = std::fs::rename(&old, app);
                return Err(e.to_string());
            }
            let _ = std::fs::remove_dir_all(&old);
            let args = if background { format!(" --args {}", crate::autostart::ARG) } else { String::new() };
            relaunch(&format!("open -n {}{args}", sh_quote(app)))
        }
        #[cfg(target_os = "linux")]
        Install::LinuxDir(dir) => {
            let tmp = std::env::temp_dir().join(format!("elo-atualizacao-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            run(Command::new("tar").arg("-xzf").arg(file).arg("-C").arg(&tmp))?;
            replace_binaries(&tmp, dir)?;
            let _ = std::fs::remove_dir_all(&tmp);
            let args = if background { format!(" {}", crate::autostart::ARG) } else { String::new() };
            relaunch(&format!("exec {}{args}", sh_quote(&dir.join("elo"))))
        }
        #[cfg(target_os = "linux")]
        Install::LinuxDeb => {
            // Pede a senha de administrador numa janela do sistema (polkit).
            run(Command::new("pkexec").arg("apt-get").arg("install").arg("-y").arg(file)).map_err(|e| format!("{e}. Instale manualmente: {}", file.display()))?;
            let args = if background { format!(" {}", crate::autostart::ARG) } else { String::new() };
            relaunch(&format!("exec /usr/bin/elo{args}"))
        }
        #[allow(unreachable_patterns)]
        _ => Err("atualização automática não disponível neste sistema".into()),
    }
}

/// Troca `elo` e `elo-server` em `dir` pelos que estão em `extracted/<pasta do pacote>/`.
/// Grava ao lado e renomeia por cima (o executável em uso continua válido até sair).
#[cfg(unix)]
pub fn replace_binaries(extracted: &Path, dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let pkg = std::fs::read_dir(extracted).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.join("elo").is_file()).ok_or("pacote sem o executável elo")?;
    for b in ["elo", "elo-server"] {
        let src = pkg.join(b);
        if !src.is_file() {
            continue;
        }
        let dst = dir.join(b);
        if b == "elo-server" && !dst.exists() {
            continue;
        }
        let tmp = dir.join(format!(".{b}.novo"));
        std::fs::copy(&src, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    }
    Ok(())
}

// ── estado no app ───────────────────────────────────────────────────

pub enum State {
    Idle,
    Checking,
    UpToDate,
    /// Versão nova; sem pacote = este jeito de instalar não se atualiza sozinho.
    Available { manifest: Manifest, asset: Option<Asset> },
    Downloading { version: String, done: Arc<AtomicU64>, total: u64 },
    Ready { version: String, file: PathBuf },
    Failed(String),
}

enum Msg {
    Checked(Result<Check, String>),
    Downloaded(String, Result<PathBuf, String>),
}

/// Verifica de tempos em tempos, baixa e avisa quando dá para instalar.
pub struct Updater {
    pub state: State,
    pub install: Install,
    rx: Option<std::sync::mpsc::Receiver<Msg>>,
    next_check: std::time::Instant,
}

impl Default for Updater {
    fn default() -> Self {
        Self { state: State::Idle, install: detect(), rx: None, next_check: std::time::Instant::now() + Duration::from_secs(20) }
    }
}

pub const RELEASES: &str = "https://github.com/Stringari-Consultoria/elo/releases/latest";

impl Updater {
    pub fn busy(&self) -> bool {
        matches!(self.state, State::Checking | State::Downloading { .. })
    }

    pub fn check_now(&mut self) {
        if self.busy() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let how = self.install.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(check(&how)));
        });
        self.rx = Some(rx);
        self.state = State::Checking;
        self.next_check = std::time::Instant::now() + Duration::from_secs(6 * 3600);
    }

    pub fn download_now(&mut self) {
        let State::Available { manifest, asset: Some(asset) } = &self.state else { return };
        let (m, a) = (manifest.clone(), asset.clone());
        let done = Arc::new(AtomicU64::new(0));
        let d2 = done.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let version = m.version.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Downloaded(m.version.clone(), download(&m, &a, &d2)));
        });
        self.rx = Some(rx);
        self.state = State::Downloading { version, done, total: asset.size };
    }

    /// Chamado a cada quadro. `auto`: baixa sozinho. Devolve `true` quando há
    /// atualização pronta e o app está livre (`idle`) para reiniciar.
    pub fn tick(&mut self, auto: bool, idle: bool) -> bool {
        if let Some(m) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            self.state = match m {
                Msg::Checked(Ok(Check::UpToDate)) => State::UpToDate,
                Msg::Checked(Ok(Check::Available(manifest, asset))) => State::Available { manifest, asset },
                Msg::Checked(Err(e)) | Msg::Downloaded(_, Err(e)) => State::Failed(e),
                Msg::Downloaded(version, Ok(file)) => State::Ready { version, file },
            };
        }
        // Só a versão instalada verifica sozinha (a de desenvolvimento não).
        let installed = !cfg!(debug_assertions);
        if auto && installed && !self.busy() && std::time::Instant::now() >= self.next_check && !matches!(self.state, State::Ready { .. }) {
            self.check_now();
        }
        if auto && matches!(&self.state, State::Available { asset: Some(_), .. }) {
            self.download_now();
        }
        auto && idle && matches!(self.state, State::Ready { .. })
    }

    /// Instala o pacote pronto. Em caso de sucesso, o app deve sair já.
    pub fn install_now(&mut self, background: bool) -> Result<(), String> {
        let State::Ready { file, .. } = &self.state else { return Err("nenhuma atualização pronta".into()) };
        let r = install(&self.install, file, background);
        if let Err(e) = &r {
            self.state = State::Failed(e.clone());
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn versions() {
        assert!(newer("0.1.2", "0.1.1"));
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(newer("v0.1.10", "0.1.9"));
        assert!(!newer("0.1.1", "0.1.1"));
        assert!(!newer("0.1.0", "0.1.1"));
    }

    /// Manifesto gerado por scripts/manifesto.py e assinado com
    /// `openssl pkeyutl -sign -rawin` e a chave real (o mesmo que o CI faz).
    #[test]
    fn real_key_and_openssl_format() {
        let body = include_bytes!("testdata/latest.json");
        let sig = include_bytes!("testdata/latest.json.sig");
        verify(body, sig, &PUBLIC_KEY).unwrap();
        let m: Manifest = serde_json::from_slice(body).unwrap();
        assert_eq!(m.version, "9.9.9");
        for k in ["windows-x64", "macos", "linux-x86_64", "linux-x86_64-deb"] {
            assert!(m.assets.contains_key(k), "{k}");
        }
        let mut ruim = body.to_vec();
        ruim[20] ^= 1;
        assert!(verify(&ruim, sig, &PUBLIC_KEY).is_err());
    }

    #[test]
    fn public_key_is_valid() {
        assert!(ed25519_dalek::VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
    }

    /// Servidor HTTP mínimo: serve os arquivos dados, um pedido por conexão.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut req = [0u8; 2048];
                let n = s.read(&mut req).unwrap_or(0);
                let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or_default().to_string();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                match files.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn manifest_signature_and_download() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = sk.verifying_key().to_bytes();
        let pacote = b"conteudo do pacote".to_vec();
        let sha: String = Sha256::digest(&pacote).iter().map(|b| format!("{b:02x}")).collect();
        let manifest = format!(r#"{{"version":"9.9.9","base_url":"BASE/","assets":{{"linux-x86_64":{{"name":"elo.tar.gz","sha256":"{sha}","size":{}}}}}}}"#, pacote.len());
        let sig = sk.sign(manifest.as_bytes()).to_bytes().to_vec();
        let mut adulterado = manifest.clone().into_bytes();
        adulterado[12] = b'8';
        let base = serve(vec![
            ("/latest.json", manifest.clone().into_bytes()),
            ("/latest.json.sig", sig.clone()),
            ("/ruim.json", adulterado),
            ("/ruim.json.sig", sig),
            ("/elo.tar.gz", pacote.clone()),
            ("/errado.tar.gz", b"outro conteudo!!!!".to_vec()),
        ]);
        // Assinatura válida.
        let m = fetch_manifest(&format!("{base}/latest.json"), &pk).unwrap();
        assert_eq!(m.version, "9.9.9");
        // Manifesto adulterado ou chave errada: recusado.
        assert!(fetch_manifest(&format!("{base}/ruim.json"), &pk).is_err());
        assert!(fetch_manifest(&format!("{base}/latest.json"), &PUBLIC_KEY).is_err());
        // Download com SHA-256 conferido.
        let a = m.assets["linux-x86_64"].clone();
        let dir = std::env::temp_dir().join(format!("elo-update-teste-{}", std::process::id()));
        let prog = Arc::new(AtomicU64::new(0));
        let p = download_to(&format!("{base}/elo.tar.gz"), &a, &dir, &prog).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), pacote);
        assert_eq!(prog.load(Ordering::Relaxed), pacote.len() as u64);
        // Conteúdo diferente do anunciado: recusado e apagado.
        assert!(download_to(&format!("{base}/errado.tar.gz"), &a, &dir, &prog).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn replaces_binaries() {
        let base = std::env::temp_dir().join(format!("elo-troca-{}", std::process::id()));
        let ext = base.join("ext/elo-9.9.9-linux-x86_64");
        let inst = base.join("bin");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(ext.join("elo"), b"novo").unwrap();
        std::fs::write(ext.join("elo-server"), b"servidor novo").unwrap();
        std::fs::write(inst.join("elo"), b"velho").unwrap();
        replace_binaries(&base.join("ext"), &inst).unwrap();
        assert_eq!(std::fs::read(inst.join("elo")).unwrap(), b"novo");
        // elo-server só é trocado se já estava instalado.
        assert!(!inst.join("elo-server").exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
