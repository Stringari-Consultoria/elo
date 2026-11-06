//! Núcleo do Elo: conexão com o servidor, sessões e acesso ao sistema.

#![forbid(unsafe_code)]

pub mod audio;
pub mod client;
pub mod net;
pub mod platform;
pub mod session;

/// Senha temporária: 8 caracteres sem ambiguidade (sem 0/O, 1/l/I).
pub fn temp_password() -> String {
    use rand::Rng;
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    (0..8).map(|_| A[r.gen_range(0..A.len())] as char).collect()
}
