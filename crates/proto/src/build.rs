// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only

//! De onde veio este executável (ver `build.rs` do crate) e onde está o
//! código-fonte dele, como pede a AGPL (seção 13 para o servidor).

/// Repositório oficial.
pub const REPO: &str = "https://github.com/Stringari-Consultoria/elo";
/// Commit do qual saiu (vazio se desconhecido).
pub const COMMIT: &str = env!("ELO_COMMIT");
/// Data do commit (AAAA-MM-DD).
pub const COMMIT_DATE: &str = env!("ELO_COMMIT_DATE");
/// Compilado pelo CI do repositório oficial (o release assinado é a prova
/// forte: o manifesto de atualização tem a assinatura Ed25519).
pub const OFFICIAL: bool = !env!("ELO_OFFICIAL").is_empty();

/// Aviso de licença gravado no executável (e mostrado no `--version`).
pub const NOTICE: &str = "Elo by Stringari (Stringari Consultoria & Tech). Software livre sob a GNU AGPL-3.0, com os termos adicionais do arquivo NOTICE: https://github.com/Stringari-Consultoria/elo";

/// Onde está o código desta versão exata.
pub fn source_url() -> String {
    if COMMIT.is_empty() { REPO.to_string() } else { format!("{REPO}/tree/{COMMIT}") }
}

/// "0.2.8 (commit 1a2b3c4d5e6f, 2026-11-17, build oficial)".
pub fn describe(version: &str) -> String {
    let mut parts = Vec::new();
    if !COMMIT.is_empty() {
        parts.push(format!("commit {COMMIT}"));
    }
    if !COMMIT_DATE.is_empty() {
        parts.push(COMMIT_DATE.to_string());
    }
    parts.push(if OFFICIAL { "build oficial".into() } else { "build não oficial".into() });
    format!("{version} ({})", parts.join(", "))
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_points_to_this_commit() {
        let url = super::source_url();
        assert!(url.starts_with(super::REPO));
        if !super::COMMIT.is_empty() {
            assert!(url.ends_with(super::COMMIT) && super::COMMIT.len() == 12);
        }
        assert!(super::describe("1.0").starts_with("1.0 ("));
    }
}
