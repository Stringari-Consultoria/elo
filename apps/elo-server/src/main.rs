//! Servidor do Elo (linha de comando). A lógica fica em `lib.rs`; o app
//! desktop embute o mesmo servidor.
//!
//! Uso: `elo-server --listen 0.0.0.0:21116 --data /var/lib/elo [--cert c.pem --key k.pem | --self-signed | --no-tls]`

use std::path::PathBuf;

use elo_server::{Config, Server, Tls};

fn args() -> Config {
    let mut listen = format!("0.0.0.0:{}", elo_proto::DEFAULT_PORT);
    let mut data = PathBuf::from("elo-dados");
    let (mut cert, mut key, mut no_tls) = (None, None, false);
    let mut retention_days = 90;
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--listen" => listen = it.next().unwrap_or(listen),
            "--data" => data = it.next().map(PathBuf::from).unwrap_or(data),
            "--cert" => cert = it.next().map(PathBuf::from),
            "--key" => key = it.next().map(PathBuf::from),
            "--no-tls" => no_tls = true,
            "--retencao-dias" => retention_days = it.next().and_then(|d| d.parse().ok()).unwrap_or(retention_days),
            "--self-signed" => {}
            "-h" | "--help" => {
                println!("elo-server --listen 0.0.0.0:21116 --data DIR [--cert C.pem --key K.pem | --self-signed | --no-tls] [--retencao-dias 90]");
                std::process::exit(0);
            }
            other => eprintln!("opção desconhecida: {other}"),
        }
    }
    let tls = match (no_tls, cert, key) {
        (true, ..) => Tls::Off,
        (false, Some(cert), Some(key)) => Tls::Files { cert, key },
        _ => Tls::SelfSigned,
    };
    Config { listen, data, tls, retention_days }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cfg = args();
    let listen = cfg.listen.clone();
    let server = Server::bind(cfg).await?;
    if let Some(fp) = &server.fingerprint {
        eprintln!("Certificado autoassinado. Impressão digital (SHA-256) para os apps: {fp}");
    }
    eprintln!("Elo servidor em {listen} ({})", if server.tls() { "TLS" } else { "sem TLS — só para testes" });
    server.run().await
}
