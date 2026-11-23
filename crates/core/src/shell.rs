// SPDX-FileCopyrightText: 2026 Gabriel Stringari
// SPDX-License-Identifier: AGPL-3.0-only
//! Terminal de texto para quem acessa, como um acesso SSH: o shell do usuário
//! daqui num pseudoterminal, com a entrada e a saída indo pelo canal cifrado
//! da sessão. Funciona sem a imagem da tela (tela gráfica travada, console de
//! texto), enquanto o Elo daqui estiver rodando.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct Shell {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    out: Receiver<Vec<u8>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.clamp(2, 500), cols: cols.clamp(10, 1000), pixel_width: 0, pixel_height: 0 }
}

impl Shell {
    /// Abre o shell padrão do usuário (de login), na pasta pessoal.
    pub fn open(cols: u16, rows: u16) -> Result<Shell, String> {
        let err = |e: &dyn std::fmt::Display| format!("não foi possível abrir o terminal: {e}");
        let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| err(&e))?;
        let mut cmd = CommandBuilder::new_default_prog();
        if let Some(home) = std::env::var_os(if cfg!(target_os = "windows") { "USERPROFILE" } else { "HOME" }) {
            cmd.cwd(home);
        }
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).map_err(|e| err(&e))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| err(&e))?;
        let writer = pair.master.take_writer().map_err(|e| err(&e))?;
        let (tx, out) = channel();
        std::thread::Builder::new()
            .name("elo-terminal".into())
            .spawn(move || {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Shell { master: pair.master, writer, child, out })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer.write_all(data).and_then(|_| self.writer.flush()).map_err(|e| format!("terminal: {e}"))
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.master.resize(size(cols, rows));
    }

    /// Saída nova (até `max` bytes) e, se o shell acabou, o motivo.
    pub fn poll(&mut self, max: usize) -> (Vec<u8>, Option<String>) {
        let mut data = Vec::new();
        while data.len() < max {
            match self.out.try_recv() {
                Ok(chunk) => data.extend_from_slice(&chunk),
                Err(TryRecvError::Empty) => return (data, None),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if data.len() >= max {
            return (data, None);
        }
        // Saída acabou: o shell saiu (exit) ou foi encerrado.
        let why = match self.child.try_wait() {
            Ok(Some(st)) if st.success() => "o terminal foi fechado".to_string(),
            Ok(Some(st)) => format!("o terminal foi fechado ({st})"),
            _ => "o terminal foi fechado".to_string(),
        };
        (data, Some(why))
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

