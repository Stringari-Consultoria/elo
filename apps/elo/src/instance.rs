//! Um Elo por configuração. Abrir de novo (atalho, menu Iniciar) com o Elo
//! já rodando na bandeja traz a janela que existe para a frente, em vez de
//! abrir um segundo Elo que brigaria pela porta do servidor embutido e pelo
//! mesmo ID.
//!
//! O primeiro escuta em 127.0.0.1 (porta qualquer) e grava a porta e um
//! segredo no arquivo `instancia` da pasta de configuração, que só o próprio
//! usuário lê. Os seguintes leem o arquivo, provam que conhecem o segredo e
//! pedem "mostrar". Configurações diferentes (Elo e Elo Cliente na mesma
//! máquina, os testes com ELO_CONFIG_DIR) rodam lado a lado.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const FILE: &str = "instancia";
const REPLY: &str = "elo";

pub enum Claim {
    /// Este é o primeiro: chame `serve` quando o app existir.
    First(Instance),
    /// Já tem um Elo aberto com esta configuração (e ele já foi avisado).
    Running,
    /// Não deu para saber (sem rede local, sem pasta): segue sem a trava.
    Unknown,
}

pub struct Instance {
    listener: TcpListener,
    secret: String,
}

fn read_file(dir: &Path) -> Option<(u16, String)> {
    let s = std::fs::read_to_string(dir.join(FILE)).ok()?;
    let mut it = s.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.to_string()))
}

/// Avisa o Elo que já está aberto. `true` se ele respondeu.
fn notify(port: u16, secret: &str, show: bool) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if writeln!(s, "{secret} {}", if show { "mostrar" } else { "nada" }).is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).is_ok() && line.trim() == REPLY
}

/// `show`: pedir para o Elo aberto mostrar a janela (falso quando este foi
/// aberto pelo início automático, que não deve trazer nada para a frente).
pub fn claim(dir: &Path, show: bool) -> Claim {
    if let Some((port, secret)) = read_file(dir)
        && notify(port, &secret, show)
    {
        return Claim::Running;
    }
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return Claim::Unknown };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return Claim::Unknown };
    let secret: String = (0..32).map(|_| format!("{:x}", rand::random::<u8>() & 0xf)).collect();
    if write_file(dir, &format!("{port} {secret}\n")).is_err() {
        return Claim::Unknown;
    }
    Claim::First(Instance { listener, secret })
}

fn write_file(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path: PathBuf = dir.join(FILE);
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)
}

impl Instance {
    /// Atende os próximos Elos: liga `show` e acorda o app quando pedirem.
    pub fn serve(self, ctx: egui::Context, show: Arc<AtomicBool>) {
        let Instance { listener, secret } = self;
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                let Ok(mut w) = s.try_clone() else { continue };
                if BufReader::new(s).read_line(&mut line).is_err() {
                    continue;
                }
                let mut it = line.split_whitespace();
                if it.next() != Some(secret.as_str()) {
                    continue;
                }
                let _ = writeln!(w, "{REPLY}");
                if it.next() == Some("mostrar") {
                    show.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_launch_shows_the_first_window() {
        let dir = std::env::temp_dir().join(format!("elo-instancia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let Claim::First(first) = claim(&dir, true) else { panic!("o primeiro tem que ficar") };
        let show = Arc::new(AtomicBool::new(false));
        first.serve(egui::Context::default(), show.clone());

        // Início automático com um Elo já aberto: sai sem trazer a janela.
        assert!(matches!(claim(&dir, false), Claim::Running));
        assert!(!show.load(Ordering::Relaxed));

        // Atalho com o Elo na bandeja: a janela que existe aparece.
        assert!(matches!(claim(&dir, true), Claim::Running));
        for _ in 0..50 {
            if show.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(show.load(Ordering::Relaxed));

        // Segredo errado (arquivo de outro usuário, porta reaproveitada): não conta.
        let (port, _) = read_file(&dir).unwrap();
        assert!(!notify(port, "segredo-errado", true));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_file_is_replaced() {
        let dir = std::env::temp_dir().join(format!("elo-instancia-velha-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Porta de um Elo que fechou sem apagar o arquivo (queda, desligamento).
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        write_file(&dir, &format!("{port} abc\n")).unwrap();
        assert!(matches!(claim(&dir, true), Claim::First(_)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
