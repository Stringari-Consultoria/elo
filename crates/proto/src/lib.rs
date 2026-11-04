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

pub const PROTOCOL_VERSION: u32 = 1;
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
