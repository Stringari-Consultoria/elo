// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Tela bloqueada no GNOME com Wayland.
//!
//! Com a tela bloqueada, o gnome-shell proíbe qualquer captura ou controle
//! remoto (`inhibit_remote_access`): ninguém de fora vê a tela de bloqueio.
//! Quem acessa digita então a senha do usuário de lá na janela do Elo; o Elo
//! confere a senha como a tela de bloqueio faria e pede ao logind para
//! desbloquear a sessão (o gnome-shell obedece ao sinal `Unlock`).
//!
//! A senha é conferida pelo `unix_chkpwd`, o ajudante do PAM que confere a
//! senha do próprio usuário sem ser administrador (só contas locais).

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ashpd::zbus::{Connection, Proxy};

const LOGIN1: &str = "org.freedesktop.login1";
/// A sessão de quem chama ou, fora de uma (o Elo aberto pelo GNOME roda num
/// escopo de app), a sessão gráfica do usuário.
const SESSION: &str = "/org/freedesktop/login1/session/auto";

/// Erros seguidos que bloqueiam as tentativas por alguns minutos.
const MAX_FAILS: usize = 5;
const FAIL_WINDOW: Duration = Duration::from_secs(300);
static FAILS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

async fn session(conn: &Connection) -> Result<Proxy<'static>, String> {
    crate::mutter::proxy(conn, LOGIN1, SESSION, "org.freedesktop.login1.Session").await
}

fn block_on<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?.block_on(f)
}

/// A tela da sessão gráfica está bloqueada (o gnome-shell avisa o logind)?
pub fn locked() -> bool {
    block_on(async {
        let conn = Connection::system().await.map_err(|e| e.to_string())?;
        session(&conn).await?.get_property::<bool>("LockedHint").await.map_err(|e| e.to_string())
    })
    .unwrap_or(false)
}

/// Onde fica o ajudante do PAM (muda de distribuição para distribuição).
fn chkpwd() -> Option<String> {
    // Testes (só em build de depuração): um conferidor de mentira.
    #[cfg(debug_assertions)]
    if let Ok(p) = std::env::var("ELO_CHKPWD") {
        return Some(p);
    }
    ["/usr/sbin/unix_chkpwd", "/sbin/unix_chkpwd", "/usr/bin/unix_chkpwd"].into_iter().find(|p| std::path::Path::new(p).exists()).map(String::from)
}

/// Confere a senha do usuário `user` (o mesmo que roda o Elo).
fn check(user: &str, password: &str) -> Result<(), String> {
    let bin = chkpwd().ok_or("este Linux não tem o unix_chkpwd para conferir a senha; desbloqueie no próprio computador")?;
    let mut child = Command::new(bin).args([user, "nonull"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("conferência da senha: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut buf = Vec::with_capacity(password.len() + 1);
        buf.extend_from_slice(password.as_bytes());
        buf.push(0);
        let r = stdin.write_all(&buf);
        buf.fill(0);
        r.map_err(|e| format!("conferência da senha: {e}"))?;
    }
    let status = child.wait().map_err(|e| format!("conferência da senha: {e}"))?;
    // Códigos do PAM: 0 = certo, 7 = senha errada.
    match status.code() {
        Some(0) => Ok(()),
        Some(7) => Err(WRONG.into()),
        c => Err(format!("não deu para conferir a senha deste usuário aqui (conta de rede?); desbloqueie no próprio computador (código {})", c.unwrap_or(-1))),
    }
}

/// Senha do Linux errada.
pub const WRONG: &str = "senha do Linux incorreta";

/// Desbloqueia a tela se `password` for a senha do usuário da sessão.
pub fn unlock(password: &str) -> Result<(), String> {
    {
        let mut f = FAILS.lock().unwrap_or_else(|e| e.into_inner());
        f.retain(|t| t.elapsed() < FAIL_WINDOW);
        if f.len() >= MAX_FAILS {
            return Err("muitas senhas erradas: o desbloqueio fica bloqueado por alguns minutos".into());
        }
    }
    block_on(async {
        let conn = Connection::system().await.map_err(|e| e.to_string())?;
        let s = session(&conn).await?;
        if !s.get_property::<bool>("LockedHint").await.unwrap_or(true) {
            return Ok(());
        }
        let user: String = s.get_property("Name").await.map_err(|e| format!("usuário da sessão: {e}"))?;
        if let Err(e) = check(&user, password) {
            if e == WRONG {
                FAILS.lock().unwrap_or_else(|e| e.into_inner()).push(Instant::now());
                // Como a tela de bloqueio: uma pausa a cada erro.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            return Err(e);
        }
        s.call::<_, _, ()>("Unlock", &()).await.map_err(|e| format!("o sistema não desbloqueou a tela: {e}"))?;
        eprintln!("[bloqueio] tela desbloqueada pelo Elo (senha do usuário {user} conferida)");
        Ok(())
    })
}
