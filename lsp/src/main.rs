use lp_lsp::Backend;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("lp-lsp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket).serve(service).await;
}
