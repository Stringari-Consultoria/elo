//! Elo (completo, para quem dá suporte) ou Elo Cliente (instalado no
//! computador do cliente: só recebe conexões e pede ajuda). É o mesmo
//! programa: o Elo Cliente vem com o executável chamado `elo-cliente`.

use std::sync::OnceLock;

/// Este executável é o Elo Cliente.
pub fn client_build() -> bool {
    static C: OnceLock<bool> = OnceLock::new();
    *C.get_or_init(|| {
        std::env::var_os("ELO_CLIENTE").is_some_and(|v| v == "1")
            || std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase())).is_some_and(|s| s.contains("cliente"))
    })
}

pub fn app_name() -> &'static str {
    if client_build() { "Elo Cliente" } else { "Elo" }
}

/// Convite deixado pela TI na máquina (instalação em massa), se houver.
pub fn invite_file() -> Option<String> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        paths.push(dir.join("convite.txt"));
    }
    if cfg!(target_os = "windows") {
        paths.push(std::path::PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join("Elo").join("convite.txt"));
    } else if cfg!(target_os = "macos") {
        paths.push("/Library/Application Support/Elo/convite.txt".into());
    } else {
        paths.push("/etc/elo/convite.txt".into());
    }
    paths.iter().filter_map(|p| std::fs::read_to_string(p).ok()).find(|t| elo_proto::ctrl::Invite::decode(t).is_some())
}
