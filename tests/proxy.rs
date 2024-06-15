use std::net::SocketAddr;

use http::StatusCode;
use sthp::proxy_request;
use tokio::net::TcpListener;

use color_eyre::Result;
use socksprox::Socks5Server;
use tokio::task::JoinHandle;

async fn start_socks_server() -> Result<(JoinHandle<()>, SocketAddr)> {
    // TODO: Currently, Socks5Server does not return the port it is bound to.
    // To work around this, we will use TcpListener to obtain a random available port.
    // After retrieving the port, we will immediately release it.
    let listener = TcpListener::bind("localhost:0").await?;
    let addr = listener.local_addr()?;
    let port = addr.port();
    eprintln!("socks proxy will listen on port {}", port);
    // release port
    drop(listener);

    let mut server = Socks5Server::new("localhost", port, None, None)
        .await
        .unwrap();
    let join_handle = tokio::task::spawn(async move {
        server.serve().await;
    });
    Ok((join_handle, addr))
}

#[tokio::test]
async fn simple_test() -> Result<()> {
    let (_, socks_proxy_addr) = start_socks_server().await?;
    let listener = TcpListener::bind("localhost:0").await?;
    let addr = listener.local_addr()?;
    let _ = tokio::task::spawn(async move {
        let (stream, proxy_addr) = listener.accept().await?;
        proxy_request(stream, socks_proxy_addr, &None, &None).await?;
        eprintln!("new connection from: {:?}", proxy_addr);
        Ok::<_, color_eyre::eyre::Error>(())
    });
    eprintln!("http proxy will listen on {}", addr);

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::http(format!(
            "http://{}:{}",
            addr.ip(),
            addr.port()
        ))?)
        .build()?;

    assert_eq!(
        client.get("http://example.org").send().await?.status(),
        StatusCode::OK
    );
    Ok(())
}
