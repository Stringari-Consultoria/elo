// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! GNOME no Wayland sem pedir na tela: a API do próprio Mutter
//! (`org.gnome.Mutter.RemoteDesktop` + `org.gnome.Mutter.ScreenCast`), a
//! mesma do compartilhamento de tela do GNOME.
//!
//! O portal do GNOME 46 pergunta na tela a cada sessão (a permissão guardada
//! volta sem a captura), e o acesso não assistido não funcionava. Aqui o
//! Elo fala direto com o Mutter, como faz no X11: quem libera o acesso é o
//! próprio Elo (senha ou aceite), e o GNOME mostra no alto da tela que ela
//! está sendo compartilhada. Fora do GNOME (KDE e outros), continua o portal.

use std::collections::HashMap;
use std::time::Duration;

use ashpd::zbus::proxy::{Builder, CacheProperties};
use ashpd::zbus::{Connection, DynamicType, ObjectPath, OwnedObjectPath, OwnedVariant as OwnedValue, Proxy, Variant as Value};
use futures_util::StreamExt;

use crate::wayland::StreamMeta;

const RD: &str = "org.gnome.Mutter.RemoteDesktop";
const SC: &str = "org.gnome.Mutter.ScreenCast";
const DC: &str = "org.gnome.Mutter.DisplayConfig";

/// Sessão aberta no Mutter: o controle e um fluxo por monitor.
pub(crate) struct Mutter {
    rd: Proxy<'static>,
    streams: Vec<OwnedObjectPath>,
}

async fn proxy(conn: &Connection, dest: &'static str, path: impl TryInto<ObjectPath<'static>, Error: Into<ashpd::zbus::Error>>, iface: &'static str) -> Result<Proxy<'static>, String> {
    Builder::new(conn).destination(dest).path(path).interface(iface).cache_properties(CacheProperties::No).build().await.map_err(|e| e.to_string())
}

/// O Mutter está nesta sessão (GNOME no Wayland)?
pub(crate) async fn present(conn: &Connection) -> bool {
    let Ok(dbus) = ashpd::zbus::fdo::DBusProxy::new(conn).await else { return false };
    let Ok(name) = RD.try_into() else { return false };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

type Ids = (String, String, String, String);
type Mode = (String, i32, i32, f64, f64, Vec<f64>, HashMap<String, OwnedValue>);
type Mon = (Ids, Vec<Mode>, HashMap<String, OwnedValue>);
type Logical = (i32, i32, f64, u32, bool, Vec<Ids>, HashMap<String, OwnedValue>);
type State = (u32, Vec<Mon>, Vec<Logical>, HashMap<String, OwnedValue>);

/// Monitores ligados, na ordem da tela (esquerda → direita, cima → baixo):
/// conector e se é o principal.
async fn connectors(conn: &Connection) -> Result<Vec<(String, bool)>, String> {
    let dc = proxy(conn, DC, "/org/gnome/Mutter/DisplayConfig", DC).await?;
    let (_, _, logical, _): State = dc.call("GetCurrentState", &()).await.map_err(|e| format!("monitores do GNOME: {e}"))?;
    let mut l = logical;
    l.sort_by_key(|m| (m.0, m.1));
    let out: Vec<(String, bool)> = l.iter().flat_map(|m| m.5.iter().map(move |ids| (ids.0.clone(), m.4))).collect();
    if out.is_empty() {
        return Err("o GNOME não informou nenhum monitor ligado".into());
    }
    Ok(out)
}

/// Abre a sessão: controle + captura de todos os monitores. Não pergunta nada na tela.
pub(crate) async fn open(conn: &Connection) -> Result<(Mutter, Vec<StreamMeta>), String> {
    let mons = connectors(conn).await?;
    let rdm = proxy(conn, RD, "/org/gnome/Mutter/RemoteDesktop", RD).await?;
    let rd_path: OwnedObjectPath = rdm.call("CreateSession", &()).await.map_err(|e| format!("sessão de acesso remoto do GNOME: {e}"))?;
    let rd = proxy(conn, RD, rd_path, "org.gnome.Mutter.RemoteDesktop.Session").await?;
    let r = async {
        let id: String = rd.get_property("SessionId").await.map_err(|e| e.to_string())?;
        let scm = proxy(conn, SC, "/org/gnome/Mutter/ScreenCast", SC).await?;
        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("remote-desktop-session-id", Value::from(id));
        let sc_path: OwnedObjectPath = scm.call("CreateSession", &(opts,)).await.map_err(|e| format!("captura do GNOME: {e}"))?;
        let sc = proxy(conn, SC, sc_path, "org.gnome.Mutter.ScreenCast.Session").await?;
        let mut streams = Vec::new();
        let mut added = Vec::new();
        for (c, _) in &mons {
            // Cursor desenhado na imagem (como no portal).
            let mut o: HashMap<&str, Value> = HashMap::new();
            o.insert("cursor-mode", Value::from(1u32));
            let path: OwnedObjectPath = sc.call("RecordMonitor", &(c.as_str(), o)).await.map_err(|e| format!("captura do monitor {c}: {e}"))?;
            let sp = proxy(conn, SC, path.clone(), "org.gnome.Mutter.ScreenCast.Stream").await?;
            // Inscreve antes do Start: o nó do PipeWire chega por sinal.
            let sig = sp.receive_signal("PipeWireStreamAdded").await.map_err(|e| e.to_string())?;
            added.push((sp, sig));
            streams.push(path);
        }
        rd.call::<_, _, ()>("Start", &()).await.map_err(|e| format!("início do acesso remoto do GNOME: {e}"))?;
        let mut meta = Vec::new();
        for (sp, mut sig) in added {
            let msg = tokio::time::timeout(Duration::from_secs(10), sig.next()).await.map_err(|_| "o GNOME não mandou a imagem da tela".to_string())?.ok_or("o GNOME fechou a captura")?;
            let node: u32 = msg.body().deserialize().map_err(|e| e.to_string())?;
            let p: HashMap<String, OwnedValue> = sp.get_property("Parameters").await.map_err(|e| e.to_string())?;
            let (w, h) = p.get("size").and_then(|v| <(i32, i32)>::try_from(v.try_clone().ok()?).ok()).unwrap_or((0, 0));
            meta.push(StreamMeta { node, width: w.max(0) as u32, height: h.max(0) as u32 });
        }
        Ok::<_, String>((meta, streams))
    }
    .await;
    match r {
        Ok((meta, streams)) => {
            eprintln!("[gnome] acesso remoto pelo Mutter ({} tela(s), sem perguntar na tela)", meta.len());
            Ok((Mutter { rd, streams }, meta))
        }
        Err(e) => {
            let _ = rd.call::<_, _, ()>("Stop", &()).await;
            Err(e)
        }
    }
}

impl Mutter {
    /// Mouse na posição (x, y) da tela `stream` (pixels dela).
    pub(crate) async fn motion(&self, stream: usize, x: f64, y: f64) -> Result<(), String> {
        let Some(s) = self.streams.get(stream).or(self.streams.first()) else { return Ok(()) };
        self.call("NotifyPointerMotionAbsolute", &(s.as_str(), x, y)).await
    }

    /// `button`: código do Linux (BTN_LEFT = 0x110...).
    pub(crate) async fn button(&self, button: i32, down: bool) -> Result<(), String> {
        self.call("NotifyPointerButton", &(button, down)).await
    }

    /// Rolagem suave em pixels (positivo = baixo/direita). Sem flags, como o
    /// portal: o GNOME trata como touchpad (a conta da rolagem fina conta com isso).
    pub(crate) async fn axis(&self, dx: f64, dy: f64) -> Result<(), String> {
        self.call("NotifyPointerAxis", &(dx, dy, 0u32)).await
    }

    /// Cliques da roda: `axis` 0 = vertical, 1 = horizontal.
    pub(crate) async fn discrete(&self, axis: u32, steps: i32) -> Result<(), String> {
        self.call("NotifyPointerAxisDiscrete", &(axis, steps)).await
    }

    pub(crate) async fn keysym(&self, sym: u32, down: bool) -> Result<(), String> {
        self.call("NotifyKeyboardKeysym", &(sym, down)).await
    }

    /// Espera o GNOME fechar a sessão por conta própria (tela bloqueada,
    /// monitor que some e volta, gnome-shell reiniciando).
    pub(crate) async fn closed(&self) {
        match self.rd.receive_signal("Closed").await {
            Ok(mut s) => {
                s.next().await;
            }
            Err(_) => std::future::pending().await,
        }
    }

    pub(crate) async fn close(&self) {
        let _ = self.rd.call::<_, _, ()>("Stop", &()).await;
    }

    async fn call<B: serde::Serialize + DynamicType>(&self, method: &'static str, body: &B) -> Result<(), String> {
        self.rd.call::<_, _, ()>(method, body).await.map_err(|e| e.to_string())
    }
}
