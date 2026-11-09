//! Servidor do Elo (linha de comando). A lógica fica em `lib.rs`; o app
//! desktop embute o mesmo servidor.
//!
//! Uso: `elo-server --listen 0.0.0.0:21116 --data /var/lib/elo [--cert c.pem --key k.pem | --self-signed | --no-tls] [--proxy-confiavel IP]`
//!      `elo-server --impressao --data /var/lib/elo` (mostra a impressão digital e sai)

use std::path::PathBuf;

use elo_server::{Config, Server, Tls};

fn args() -> Config {
    let mut listen = format!("0.0.0.0:{}", elo_proto::DEFAULT_PORT);
    let mut data = PathBuf::from("elo-dados");
    let (mut cert, mut key, mut no_tls) = (None, None, false);
    let mut retention_days = 90;
    let mut show_fp = false;
    let mut trusted_proxies = Vec::new();
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
            "--proxy-confiavel" => trusted_proxies.extend(it.next()),
            "--impressao" => show_fp = true,
            "-V" | "--version" => {
                println!("elo-server {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                println!("elo-server {}", env!("CARGO_PKG_VERSION"));
                println!("  elo-server --listen 0.0.0.0:21116 --data DIR [--cert C.pem --key K.pem | --self-signed | --no-tls] [--retencao-dias 90]");
                println!("  elo-server --impressao --data DIR    mostra a impressão digital do certificado autoassinado");
                println!("  --proxy-confiavel IP|REDE            proxy (ex.: cloudflared no Docker) que pode falar sem TLS e informar o IP real; repita para vários");
                std::process::exit(0);
            }
            other => eprintln!("opção desconhecida: {other}"),
        }
    }
    if show_fp {
        match elo_server::self_signed_fingerprint(&data) {
            Some(fp) => {
                println!("{fp}");
                std::process::exit(0);
            }
            None => {
                eprintln!("ainda não há certificado autoassinado em {}: inicie o servidor uma vez com --self-signed", data.display());
                std::process::exit(1);
            }
        }
    }
    let tls = match (no_tls, cert, key) {
        (true, ..) => Tls::Off,
        (false, Some(cert), Some(key)) => Tls::Files { cert, key },
        _ => Tls::SelfSigned,
    };
    Config { listen, data, tls, retention_days, trusted_proxies }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cfg = args();
    let listen = cfg.listen.clone();
    let server = Server::bind(cfg).await?;
    if let Some(fp) = &server.fingerprint {
        eprintln!("Certificado autoassinado. Impressão digital (SHA-256) para os apps: {fp}");
    }
    eprintln!("Elo servidor em {listen} ({})", if server.tls() { "TLS" } else { "sem TLS: só aceita o próprio computador e os proxies confiáveis" });
    server.run().await
}
