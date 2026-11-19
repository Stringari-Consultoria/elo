// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Enquanto alguém acessa este computador, a tela não bloqueia nem apaga por
//! falta de uso: quem só está olhando (sem mexer o mouse) não perde a imagem
//! nem precisa digitar a senha de novo. Pede ao sistema pelo
//! `org.freedesktop.ScreenSaver` (GNOME, KDE e outros); o pedido vale
//! enquanto a conexão com o D-Bus estiver aberta, então some sozinho se o
//! Elo fechar.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

const NAME: &str = "org.freedesktop.ScreenSaver";
const PATH: &str = "/org/freedesktop/ScreenSaver";

/// Acessos em andamento e o aviso para a thread soltar o pedido.
static STATE: Mutex<(usize, Option<Sender<()>>)> = Mutex::new((0, None));

/// Um acesso em andamento. O primeiro pede ao sistema; o último solta.
pub struct Awake(());

pub fn keep_awake() -> Awake {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.0 += 1;
    if st.0 == 1 {
        let (tx, rx) = channel::<()>();
        st.1 = Some(tx);
        let _ = std::thread::Builder::new().name("elo-acordado".into()).spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            rt.block_on(async move {
                let r = async {
                    let conn = ashpd::zbus::Connection::session().await.map_err(|e| e.to_string())?;
                    let p = crate::mutter::proxy(&conn, NAME, PATH, NAME).await?;
                    let cookie: u32 = p.call("Inhibit", &("Elo", "acesso remoto em andamento")).await.map_err(|e| e.to_string())?;
                    Ok::<_, String>((conn, p, cookie))
                }
                .await;
                match r {
                    Ok((_conn, p, cookie)) => {
                        // Espera o último acesso acabar (sem prender o runtime).
                        let _ = tokio::task::spawn_blocking(move || rx.recv()).await;
                        let _ = p.call::<_, _, ()>("UnInhibit", &(cookie,)).await;
                    }
                    Err(e) => eprintln!("[tela] não deu para impedir o bloqueio por falta de uso durante o acesso: {e}"),
                }
            });
        });
    }
    Awake(())
}

impl Drop for Awake {
    fn drop(&mut self) {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        st.0 = st.0.saturating_sub(1);
        if st.0 == 0
            && let Some(tx) = st.1.take()
        {
            let _ = tx.send(());
        }
    }
}
