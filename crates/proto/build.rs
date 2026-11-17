// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only

//! De onde veio este executável: commit e data dele (não a hora da
//! compilação, para o build continuar reproduzível) e se saiu do CI do
//! repositório oficial. Aparece no `--version` e na tela Sobre.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

fn main() {
    // Sem .git (ex.: imagem Docker), o commit pode vir de fora.
    let commit = std::env::var("ELO_COMMIT").ok().filter(|s| !s.is_empty()).or_else(|| git(&["rev-parse", "--short=12", "HEAD"])).unwrap_or_default();
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_default();
    let official = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") && std::env::var("GITHUB_REPOSITORY").as_deref() == Ok("Stringari-Consultoria/elo");
    println!("cargo:rustc-env=ELO_COMMIT={commit}");
    println!("cargo:rustc-env=ELO_COMMIT_DATE={date}");
    println!("cargo:rustc-env=ELO_OFFICIAL={}", if official { "1" } else { "" });
    println!("cargo:rerun-if-env-changed=ELO_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_REPOSITORY");
    // Recompila só quando o commit muda (não a cada arquivo editado).
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={dir}/{r}");
        }
    }
}
