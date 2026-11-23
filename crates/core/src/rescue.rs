// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Socorro quando a tela daqui trava (Linux), pedido por quem acessa:
//!
//! - console de texto (o Ctrl+Alt+F3) e a volta para a tela gráfica, pelo
//!   logind (as teclas mandadas de longe não chegam ao sistema com a tela
//!   gráfica travada);
//! - reiniciar só a sessão: sai dela e o login automático entra de novo, com
//!   o Elo junto;
//! - reinício de emergência: Alt+SysRq S, U, B por um teclado virtual do
//!   kernel (uinput), para quando o sistema não reinicia do jeito normal.

use std::time::Duration;

use ashpd::zbus::Connection;
use elo_proto::msg::Rescue;

const LOGIN1: &str = "org.freedesktop.login1";
const SESSION: &str = "/org/freedesktop/login1/session/auto";
const SEAT: &str = "/org/freedesktop/login1/seat/auto";

fn block_on<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?.block_on(f)
}

/// Confere antes se o pedido pode dar certo (os que derrubam o Elo daqui não
/// teriam como avisar depois).
pub fn check(r: Rescue) -> Result<(), String> {
    match r {
        Rescue::RestartSession => {
            let user = session_user()?;
            let conf = ["/etc/gdm3/custom.conf", "/etc/gdm/custom.conf"].into_iter().find_map(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
            if autologin_for(&conf, &user) { Ok(()) } else { Err(NO_AUTOLOGIN.into()) }
        }
        Rescue::EmergencyReboot => emergency_ready(),
        Rescue::TextConsole | Rescue::GraphicConsole => Ok(()),
    }
}

fn session_user() -> Result<String, String> {
    block_on(async {
        let conn = Connection::system().await.map_err(|e| e.to_string())?;
        let s = crate::mutter::proxy(&conn, LOGIN1, SESSION, "org.freedesktop.login1.Session").await?;
        s.get_property::<String>("Name").await.map_err(|e| format!("usuário da sessão: {e}"))
    })
}

/// Faz o pedido. `RestartSession` e `EmergencyReboot` derrubam o Elo daqui.
pub fn run(r: Rescue) -> Result<(), String> {
    match r {
        Rescue::TextConsole => block_on(async {
            let conn = Connection::system().await.map_err(|e| e.to_string())?;
            let s = crate::mutter::proxy(&conn, LOGIN1, SESSION, "org.freedesktop.login1.Session").await?;
            let mine: u32 = s.get_property("VTNr").await.unwrap_or(0);
            // O terminal 3, a não ser que a sessão daqui seja nele.
            let vt: u32 = if mine == 3 { 4 } else { 3 };
            let seat = crate::mutter::proxy(&conn, LOGIN1, SEAT, "org.freedesktop.login1.Seat").await?;
            seat.call::<_, _, ()>("SwitchTo", &(vt,)).await.map_err(|e| format!("não deu para ir para o console de texto: {e}"))?;
            eprintln!("[socorro] console de texto {vt} (a pedido de quem acessa)");
            Ok(())
        }),
        Rescue::GraphicConsole => block_on(async {
            let conn = Connection::system().await.map_err(|e| e.to_string())?;
            let s = crate::mutter::proxy(&conn, LOGIN1, SESSION, "org.freedesktop.login1.Session").await?;
            s.call::<_, _, ()>("Activate", &()).await.map_err(|e| format!("não deu para voltar para a tela gráfica: {e}"))?;
            eprintln!("[socorro] de volta para a tela gráfica (a pedido de quem acessa)");
            Ok(())
        }),
        Rescue::RestartSession => {
            check(r)?;
            let user = session_user()?;
            eprintln!("[socorro] reiniciando a sessão de {user} (a pedido de quem acessa)");
            block_on(async {
                let conn = Connection::system().await.map_err(|e| e.to_string())?;
                let s = crate::mutter::proxy(&conn, LOGIN1, SESSION, "org.freedesktop.login1.Session").await?;
                s.call::<_, _, ()>("Terminate", &()).await.map_err(|e| format!("o sistema não encerrou a sessão: {e}"))
            })
        }
        Rescue::EmergencyReboot => emergency_reboot(),
    }
}

/// Sem o login automático, a sessão não volta e o Elo também não.
pub const NO_AUTOLOGIN: &str = "o login automático não está ligado neste computador: depois de sair, ele ficaria na tela de login e o Elo não voltaria. Ligue em Configurações → Acesso não assistido, ou use Reiniciar o computador";

/// O GDM entra sozinho com `user` (`custom.conf`, seção `[daemon]`)?
fn autologin_for(conf: &str, user: &str) -> bool {
    let mut daemon = false;
    let (mut auto_on, mut auto_user, mut timed_on, mut timed_user) = (false, String::new(), false, String::new());
    for line in conf.lines().map(str::trim) {
        if line.starts_with('[') {
            daemon = line.eq_ignore_ascii_case("[daemon]");
            continue;
        }
        if !daemon || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "AutomaticLoginEnable" => auto_on = v.eq_ignore_ascii_case("true"),
            "AutomaticLogin" => auto_user = v.to_string(),
            "TimedLoginEnable" => timed_on = v.eq_ignore_ascii_case("true"),
            "TimedLogin" => timed_user = v.to_string(),
            _ => {}
        }
    }
    (auto_on && auto_user == user) || (timed_on && timed_user == user)
}

/// O kernel aceita S (gravar), U (proteger o disco) e B (reiniciar) pelo
/// SysRq? `kernel.sysrq`: 1 = tudo; senão, os bits 16, 32 e 128.
fn sysrq_allows(mask: u32) -> bool {
    mask == 1 || (mask & (16 | 32 | 128)) == (16 | 32 | 128)
}

/// Dá para fazer o reinício de emergência daqui? (Motivo, se não.)
pub fn emergency_ready() -> Result<(), String> {
    let mask: u32 = std::fs::read_to_string("/proc/sys/kernel/sysrq").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    if !sysrq_allows(mask) {
        return Err(format!("este Linux não libera as teclas S, U e B do SysRq (kernel.sysrq = {mask})"));
    }
    if !crate::uinput::available() {
        return Err("o Elo não tem acesso ao teclado virtual (/dev/uinput) neste computador".into());
    }
    Ok(())
}

fn emergency_reboot() -> Result<(), String> {
    use evdev::uinput::VirtualDevice;
    use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
    emergency_ready()?;
    let mut dev = VirtualDevice::builder()
        .and_then(|b| b.name("Elo (socorro)").with_keys(&AttributeSet::from_iter([KeyCode::KEY_LEFTALT, KeyCode::KEY_SYSRQ, KeyCode::KEY_S, KeyCode::KEY_U, KeyCode::KEY_B])))
        .and_then(|b| b.build())
        .map_err(|e| format!("teclado virtual: {e}"))?;
    // O kernel precisa ver o teclado novo antes das teclas.
    std::thread::sleep(Duration::from_millis(500));
    eprintln!("[socorro] reinício de emergência (Alt+SysRq S, U, B) a pedido de quem acessa");
    let key = |dev: &mut VirtualDevice, k: KeyCode, v: i32| dev.emit(&[InputEvent::new(EventType::KEY.0, k.0, v)]).map_err(|e| format!("teclado virtual: {e}"));
    for (letter, wait) in [(KeyCode::KEY_S, 3), (KeyCode::KEY_U, 3), (KeyCode::KEY_B, 0)] {
        key(&mut dev, KeyCode::KEY_LEFTALT, 1)?;
        key(&mut dev, KeyCode::KEY_SYSRQ, 1)?;
        key(&mut dev, letter, 1)?;
        key(&mut dev, letter, 0)?;
        key(&mut dev, KeyCode::KEY_SYSRQ, 0)?;
        key(&mut dev, KeyCode::KEY_LEFTALT, 0)?;
        // Dá tempo de gravar e de proteger o disco antes do próximo passo.
        std::thread::sleep(Duration::from_secs(wait));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autologin_only_for_this_user_in_daemon_section() {
        let on = "[daemon]\n# AutomaticLoginEnable=true\nTimedLoginEnable=true\nTimedLogin=gabriel\nTimedLoginDelay=10\n[security]\n";
        assert!(autologin_for(on, "gabriel"));
        assert!(!autologin_for(on, "outro"));
        assert!(autologin_for("[daemon]\nAutomaticLoginEnable = True\nAutomaticLogin = ana\n", "ana"));
        // Comentado, desligado ou em outra seção: não vale.
        assert!(!autologin_for("[daemon]\n#TimedLoginEnable=true\n#TimedLogin=gabriel\n", "gabriel"));
        assert!(!autologin_for("[daemon]\nTimedLoginEnable=false\nTimedLogin=gabriel\n", "gabriel"));
        assert!(!autologin_for("[xdmcp]\nTimedLoginEnable=true\nTimedLogin=gabriel\n", "gabriel"));
    }

    #[test]
    fn sysrq_needs_sync_remount_and_reboot() {
        assert!(sysrq_allows(1));
        assert!(sysrq_allows(176));
        assert!(!sysrq_allows(0));
        // Ubuntu padrão (176 sem o reboot seria 48): não dá.
        assert!(!sysrq_allows(48));
        assert!(!sysrq_allows(438 & !128));
    }
}
