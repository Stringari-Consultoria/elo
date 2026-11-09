//! Protocolo do Elo.
//!
//! - `frame`: quadros com prefixo de tamanho sobre qualquer `Read`/`Write`.
//! - `ctrl`: mensagens entre o app e o servidor (registro, ID, convite, relay).
//! - `secure`: aperto de mão SPAKE2 (senha nunca trafega; o servidor não
//!   consegue ler nem se passar por um dos lados) e canal ChaCha20-Poly1305.
//! - `msg`: mensagens da sessão (tela, entrada, área de transferência, chat, arquivos).
//! - `codec`: tela em blocos de 64 px, só os que mudaram, em JPEG.

#![forbid(unsafe_code)]

pub mod codec;
pub mod ctrl;
pub mod frame;
pub mod msg;
pub mod secure;

/// 2: gerenciador de arquivos (ListDir, DirListing, FileRequest, Notice).
pub const PROTOCOL_VERSION: u32 = 2;
pub const DEFAULT_PORT: u16 = 21116;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("conexão: {0}")]
    Io(#[from] std::io::Error),
    #[error("mensagem inválida: {0}")]
    Decode(#[from] postcard::Error),
    #[error("autenticação recusada")]
    Auth,
    #[error("integridade da mensagem falhou (canal adulterado ou chave errada)")]
    Crypto,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Texto vindo do outro lado para mostrar numa linha (nome, sistema): sem
/// quebras, sem caracteres de controle e sem os invisíveis que invertem a
/// ordem das letras ("fatura\u{202E}fdp.exe" aparece como "fatura exe.pdf").
pub fn plain_text(s: &str, max: usize) -> String {
    let invisible = |c: char| matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}');
    let mut out = String::new();
    for c in s.chars().filter(|c| !invisible(*c)) {
        let c = if c.is_control() || c.is_whitespace() { ' ' } else { c };
        // Espaços repetidos viram um (nome com 500 espaços empurra os botões para fora).
        if c == ' ' && (out.is_empty() || out.ends_with(' ')) {
            continue;
        }
        out.push(c);
    }
    out.trim_end().chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_text_has_no_tricks() {
        assert_eq!(super::plain_text("fatura\u{202E}fdp.exe", 64), "faturafdp.exe");
        assert_eq!(super::plain_text("  Carlos\n(técnico)\t\t ok  ", 64), "Carlos (técnico) ok");
        assert_eq!(super::plain_text(&"a".repeat(100), 64).len(), 64);
        assert_eq!(super::plain_text("\u{200B}\u{FEFF}", 64), "");
    }
}
