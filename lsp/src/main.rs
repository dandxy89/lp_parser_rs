use tower_lsp_server::Server;

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("lp-lsp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (service, socket) = lp_lsp::service();
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket).serve(service).await;
}
