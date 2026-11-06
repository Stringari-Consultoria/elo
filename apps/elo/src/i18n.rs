//! Idioma da interface: português (o texto do código) ou inglês.
//!
//! `t!("texto")` devolve o texto no idioma escolhido; com argumentos,
//! `t!("{} pediu ajuda", nome)` preenche os `{}` na ordem (o inglês pode
//! reordenar a frase, mas mantém a mesma quantidade de `{}`).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static INGLES: AtomicBool = AtomicBool::new(false);

/// `"auto"` (o idioma do sistema), `"pt"` ou `"en"`.
pub fn set(lang: &str) {
    let en = match lang {
        "en" => true,
        "pt" => false,
        _ => !system_is_portuguese(),
    };
    INGLES.store(en, Ordering::Relaxed);
}

pub fn english() -> bool {
    INGLES.load(Ordering::Relaxed)
}

/// Português no sistema? (Qualquer outro idioma usa o inglês.)
fn system_is_portuguese() -> bool {
    sys_locale::get_locale().is_none_or(|l| l.to_lowercase().starts_with("pt"))
}

fn dicionario() -> &'static HashMap<&'static str, &'static str> {
    static D: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    D.get_or_init(|| crate::i18n_en::EN.iter().copied().collect())
}

/// O texto no idioma da interface (o próprio texto se faltar tradução).
pub fn tr(pt: &'static str) -> &'static str {
    if !english() {
        return pt;
    }
    dicionario().get(pt).copied().unwrap_or(pt)
}

/// Preenche os `{}` do modelo na ordem.
pub fn fill(model: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(model.len() + 16);
    let mut it = args.iter();
    let mut rest = model;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().map(String::as_str).unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out.replace("{{", "{").replace("}}", "}")
}

/// Texto que veio de fora (núcleo, servidor): traduz se for conhecido.
pub fn tr_dyn(pt: &str) -> String {
    if !english() {
        return pt.to_string();
    }
    if let Some(s) = dicionario().get(pt) {
        return s.to_string();
    }
    // Mensagem com partes variáveis (endereço, erro do sistema): troca os trechos conhecidos.
    crate::i18n_en::PARTS
        .iter()
        .fold(pt.to_string(), |s, (p, e)| s.replace(p, e))
}

macro_rules! t {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($a:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$(::std::string::ToString::to_string(&$a)),+])
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn fills_in_order() {
        assert_eq!(
            super::fill("{} pediu ajuda: {}", &["Ana".into(), "impressora".into()]),
            "Ana pediu ajuda: impressora"
        );
        assert_eq!(
            super::fill("chaves {{assim}} {}", &["x".into()]),
            "chaves {assim} x"
        );
    }

    #[test]
    fn translates_parts() {
        super::INGLES.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            super::tr_dyn(
                "TLS com 1.2.3.4:21116: certificado do servidor não confere com a impressão digital configurada"
            ),
            "TLS with 1.2.3.4:21116: server certificate does not match the configured fingerprint"
        );
        super::INGLES.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn same_placeholders() {
        for (pt, en) in crate::i18n_en::EN {
            assert_eq!(pt.matches("{}").count(), en.matches("{}").count(), "{pt}");
        }
    }
}
