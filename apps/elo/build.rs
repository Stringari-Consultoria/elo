//! No Windows, embute o ícone e os dados do produto no elo.exe.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/elo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/icons/elo.ico");
    res.set("ProductName", "Elo");
    res.set("FileDescription", "Elo - acesso remoto");
    res.set("CompanyName", "Stringari Consultoria");
    res.set("LegalCopyright", "(c) 2026 Gabriel Stringari. MIT ou Apache-2.0.");
    if let Err(e) = res.compile() {
        // Sem o compilador de recursos, o app funciona; só fica sem ícone.
        println!("cargo:warning=ícone do Windows não embutido: {e}");
    }
}
