//! Linux com GDM (Ubuntu, Zorin, Fedora...): entrar sozinho quando a tela de
//! login aparece. Se a sessão cair (no Wayland, uma quebra do gnome-shell
//! derruba a sessão inteira) ou o computador reiniciar, ele volta para o
//! usuário em 10 s, o Elo abre junto (início automático) e o acesso não
//! assistido funciona de novo, sem ninguém na frente da tela.
//!
//! Usa o `TimedLogin` do GDM em `custom.conf`. Mexer nele exige
//! administrador: o Elo se chama de novo pelo `pkexec` (o sistema pede a
//! senha numa janela) com [`ARG`], e essa execução só troca as chaves
//! `TimedLogin*` da seção `[daemon]`, para o usuário que pediu (o
//! `PKEXEC_UID`, não um nome vindo de fora). O GDM só lê o arquivo quando
//! inicia: vale a partir do próximo reinício.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

/// Argumento da execução como administrador: `--gdm-entrada-automatica on|off`.
pub const ARG: &str = "--gdm-entrada-automatica";
/// Segundos na tela de login antes de entrar.
const DELAY: u32 = 10;
const KEYS: [&str; 3] = ["TimedLoginEnable", "TimedLogin", "TimedLoginDelay"];

/// Arquivo do GDM nesta máquina (`None`: sem GDM, a opção não aparece).
pub fn conf_path() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ["/etc/gdm3/custom.conf", "/etc/gdm/custom.conf"].into_iter().map(PathBuf::from).find(|p| p.exists())
}

fn user_name() -> Option<String> {
    std::env::var("USER").ok().filter(|u| !u.is_empty())
}

/// Ligado para o usuário atual?
pub fn enabled() -> bool {
    let (Some(p), Some(u)) = (conf_path(), user_name()) else { return false };
    std::fs::read_to_string(p).is_ok_and(|s| is_on(&s, &u))
}

/// Chave de uma linha `Chave=valor` (também comentada com `#`).
fn key_of(line: &str) -> Option<&str> {
    let t = line.trim_start().trim_start_matches('#').trim_start();
    let (k, _) = t.split_once('=')?;
    Some(k.trim())
}

/// A entrada temporizada está ligada para `user` na seção `[daemon]`?
pub fn is_on(conf: &str, user: &str) -> bool {
    let (mut section, mut enable, mut who) = (String::new(), false, false);
    for l in conf.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            section = t.to_string();
            continue;
        }
        if section != "[daemon]" || t.starts_with('#') {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        match k.trim() {
            "TimedLoginEnable" => enable = v.trim().eq_ignore_ascii_case("true"),
            "TimedLogin" => who = v.trim() == user,
            _ => {}
        }
    }
    enable && who
}

/// O arquivo com a entrada temporizada ligada para `user` (`Some`) ou
/// desligada (`None`). Tira as linhas `TimedLogin*` (as de exemplo,
/// comentadas, também) e mantém todo o resto como estava.
pub fn edit(conf: &str, user: Option<&str>) -> String {
    let block = |u: &str| ["TimedLoginEnable=true".to_string(), format!("TimedLogin={u}"), format!("TimedLoginDelay={DELAY}")];
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for l in conf.lines() {
        if key_of(l).is_some_and(|k| KEYS.contains(&k)) {
            continue;
        }
        out.push(l.to_string());
        if !placed && l.trim() == "[daemon]" {
            placed = true;
            if let Some(u) = user {
                out.extend(block(u));
            }
        }
    }
    if !placed && let Some(u) = user {
        let mut head = vec!["[daemon]".to_string()];
        head.extend(block(u));
        head.push(String::new());
        out.splice(0..0, head);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Pede ao sistema (com a senha de administrador) para ligar ou desligar.
/// Roda numa thread: o `pkexec` espera a pessoa responder.
pub fn request(on: bool) -> Receiver<Result<bool, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = (|| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
            let st = std::process::Command::new("pkexec")
                .arg(exe)
                .arg(ARG)
                .arg(if on { "on" } else { "off" })
                .status()
                .map_err(|e| format!("pkexec: {e}"))?;
            match st.code() {
                Some(0) => Ok(on),
                // 126: a pessoa fechou a janela; 127: senha recusada.
                Some(126) | Some(127) => Err(t!("a senha de administrador não foi confirmada").into()),
                _ => Err(t!("o sistema não deixou mudar a entrada automática").into()),
            }
        })();
        let _ = tx.send(r);
    });
    rx
}

/// Nome do usuário com este UID (de /etc/passwd).
fn user_of_uid(uid: &str) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() > 2 && f[2] == uid).then(|| f[0].to_string())
    })
}

fn apply(path: &Path, on: bool) -> Result<(), String> {
    // Só pelo pkexec: o usuário é quem pediu, nunca o root nem um nome de fora.
    let uid = std::env::var("PKEXEC_UID").map_err(|_| "rode pelo pkexec".to_string())?;
    if uid == "0" {
        return Err("a entrada automática não pode ser do root".into());
    }
    let user = user_of_uid(&uid).ok_or("usuário desconhecido")?;
    let old = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = edit(&old, on.then_some(user.as_str()));
    if new == old {
        return Ok(());
    }
    // Cópia do original na primeira vez.
    let backup = path.with_extension("conf.antes-do-elo");
    if !backup.exists() {
        std::fs::write(&backup, &old).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("conf.elo-tmp");
    std::fs::write(&tmp, new).map_err(|e| e.to_string())?;
    if let Ok(m) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, m.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Execução como administrador (`elo --gdm-entrada-automatica on|off`):
/// devolve o código de saída, ou `None` se não for esse o pedido.
pub fn cli() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(ARG) {
        return None;
    }
    let on = match args.next().as_deref() {
        Some("on") => true,
        Some("off") => false,
        _ => return Some(2),
    };
    let Some(path) = conf_path() else {
        eprintln!("GDM não encontrado");
        return Some(1);
    };
    match apply(&path, on) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("{e}");
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UBUNTU: &str = "# GDM configuration storage\n\n[daemon]\n# Uncomment the line below to force the login screen to use Xorg\n#WaylandEnable=false\n\n# Enabling automatic login\n#  AutomaticLoginEnable = true\n#  AutomaticLogin = user1\n\n# Enabling timed login\n#  TimedLoginEnable = true\n#  TimedLogin = user1\n#  TimedLoginDelay = 10\n\n[security]\n\n[xdmcp]\n";

    #[test]
    fn turns_on_and_off_keeping_the_rest() {
        assert!(!is_on(UBUNTU, "gabriel"));
        let on = edit(UBUNTU, Some("gabriel"));
        assert!(is_on(&on, "gabriel"));
        assert!(!is_on(&on, "outra"));
        assert!(on.contains("[daemon]\nTimedLoginEnable=true\nTimedLogin=gabriel\nTimedLoginDelay=10\n"));
        // O resto continua igual (só saem as linhas de exemplo do TimedLogin).
        assert!(on.contains("#WaylandEnable=false") && on.contains("#  AutomaticLogin = user1") && on.contains("[xdmcp]"));
        // Ligar de novo não duplica.
        assert_eq!(edit(&on, Some("gabriel")), on);
        let off = edit(&on, None);
        assert!(!is_on(&off, "gabriel"));
        assert!(!off.contains("TimedLogin"));
        assert!(off.contains("[security]"));
    }

    #[test]
    fn creates_the_daemon_section_and_ignores_other_sections() {
        let on = edit("[security]\n", Some("ana"));
        assert!(on.starts_with("[daemon]\nTimedLoginEnable=true\nTimedLogin=ana\n"));
        assert!(is_on(&on, "ana"));
        // Chaves fora do [daemon] ou comentadas não contam.
        assert!(!is_on("[security]\nTimedLoginEnable=true\nTimedLogin=ana\n", "ana"));
        assert!(!is_on("[daemon]\n#TimedLoginEnable=true\nTimedLogin=ana\n", "ana"));
        assert!(is_on("[daemon]\nTimedLoginEnable = True\nTimedLogin = ana\n", "ana"));
    }
}
