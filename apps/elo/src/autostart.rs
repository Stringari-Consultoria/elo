//! Iniciar o Elo junto com o computador, por usuário e sem administrador:
//!
//! - Linux: `~/.config/autostart/br.com.stringari.elo.desktop` (XDG);
//! - macOS: `~/Library/LaunchAgents/br.com.stringari.elo.plist` (LaunchAgent);
//! - Windows: valor `Elo` em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! O Elo aberto assim recebe [`ARG`] e começa minimizado.

use std::path::PathBuf;

/// Argumento da abertura automática.
pub const ARG: &str = "--segundo-plano";

#[cfg(any(target_os = "linux", target_os = "macos"))]
const ID: &str = "br.com.stringari.elo";

fn exe() -> Result<PathBuf, String> {
    let p = std::env::current_exe().map_err(|e| format!("caminho do Elo: {e}"))?;
    // No Windows, canonicalize gera "\\?\C:\..."; lá o caminho já é absoluto.
    #[cfg(not(target_os = "windows"))]
    let p = std::fs::canonicalize(&p).unwrap_or(p);
    // Rodando de dentro do .dmg ainda montado: o caminho some ao ejetar.
    #[cfg(target_os = "macos")]
    if p.starts_with("/Volumes") {
        return Err("arraste o Elo para Aplicativos e abra de lá antes de ligar esta opção".into());
    }
    Ok(p)
}

/// Liga ou desliga.
pub fn set(on: bool) -> Result<(), String> {
    imp::set(on)
}

/// Se está ligado (consulta o sistema).
pub fn enabled() -> bool {
    imp::enabled()
}

/// Atualiza o caminho registrado se o Elo mudou de lugar (ex.: atualização).
pub fn refresh() {
    if enabled() {
        let _ = set(true);
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))?;
        Some(base.join("autostart").join(format!("{ID}.desktop")))
    }

    /// Aspas da especificação de .desktop (Exec).
    fn quote(s: &str) -> String {
        let mut q = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    }

    pub fn set(on: bool) -> Result<(), String> {
        let f = file().ok_or("pasta de configuração do usuário não encontrada")?;
        if !on {
            return match std::fs::remove_file(&f) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{}: {e}", f.display())),
            };
        }
        let exe = exe()?;
        let data = format!(
            "[Desktop Entry]\nType=Application\nName=Elo\nComment=Acesso remoto\nExec={} {ARG}\nIcon={ID}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            quote(&exe.to_string_lossy())
        );
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&f, data).map_err(|e| format!("{}: {e}", f.display()))
    }

    pub fn enabled() -> bool {
        file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|t| !t.lines().any(|l| l.trim() == "Hidden=true"))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    fn file() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.home_dir().join("Library/LaunchAgents").join(format!("{ID}.plist")))
    }

    fn xml(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    pub fn set(on: bool) -> Result<(), String> {
        let f = file().ok_or("pasta do usuário não encontrada")?;
        if !on {
            return match std::fs::remove_file(&f) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{}: {e}", f.display())),
            };
        }
        let exe = exe()?;
        let data = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{ID}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{ARG}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
            xml(&exe.to_string_lossy())
        );
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&f, data).map_err(|e| format!("{}: {e}", f.display()))
    }

    pub fn enabled() -> bool {
        file().is_some_and(|f| f.exists())
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Elo";

    pub fn set(on: bool) -> Result<(), String> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(|e| format!("registro: {e}"))?;
        if !on {
            return match key.delete_value(NAME) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("registro: {e}")),
            };
        }
        let value = format!("\"{}\" {ARG}", exe()?.display());
        key.set_value(NAME, &value).map_err(|e| format!("registro: {e}"))
    }

    pub fn enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ).and_then(|k| k.get_value::<String, _>(NAME)).is_ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod imp {
    pub fn set(_: bool) -> Result<(), String> {
        Err("não disponível neste sistema".into())
    }
    pub fn enabled() -> bool {
        false
    }
}
