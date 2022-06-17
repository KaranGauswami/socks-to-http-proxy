use anyhow::Result;
use clap::Parser;
use http::Uri;
use hyper::client::HttpConnector;
use hyper::service::{make_service_fn, service_fn};
use hyper::upgrade::Upgraded;
use hyper::{Body, Request, Response, Server};
use hyper_socks2::{Auth, SocksConnector};
use log::debug;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::{IntoTargetAddr, ToProxyAddrs};

#[derive(Parser, Debug)]
#[clap(name = "sthp", about = "Convert Socks5 proxy into Http proxy")]
struct Cli {
    #[clap(short, long, default_value = "8080")]
    /// port where Http proxy should listen
    port: u16,

    /// Socks5 proxy address
    #[clap(short, long, default_value = "127.0.0.1:1080")]
    socks_address: SocketAddr,

    /// Socks5 username
    #[clap(short = 'u', long)]
    username: Option<String>,

    /// Socks5 password
    #[clap(short = 'P', long)]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Cli::parse();
    let socks_address = args.socks_address;
    let port = args.port;

    let username = args.username;
    let password = args.password;
    let auth = if let (Some(username), Some(password)) = (username, password) {
        Some(Auth::new(username, password))
    } else {
        None
    };
    let auth = &*Box::leak(Box::new(auth));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let make_service = make_service_fn(move |_| async move {
        Ok::<_, Infallible>(service_fn(move |req| proxy(req, socks_address, auth)))
    });
    let server = Server::bind(&addr)
        .http1_preserve_header_case(true)
        .http1_title_case_headers(true)
        .serve(make_service);
    debug!("Server is listening on http://{}", addr);
    if let Err(e) = server.await {
        debug!("server error: {}", e);
    };
    Ok(())
}
fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}
async fn proxy(
    req: Request<Body>,
    socks_address: SocketAddr,
    auth: &'static Option<Auth>,
) -> Result<Response<Body>> {
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    let proxy_addr = Box::leak(Box::new(format!("socks://{}", socks_address)));
    let proxy_addr = Uri::from_static(proxy_addr);
    if let Some(plain) = host_addr(req.uri()) {
        if req.method() == hyper::Method::CONNECT {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, plain, socks_address, auth).await {
                            debug!("server io error: {}", e);
                        };
                    }
                    Err(e) => debug!("upgrade error: {}", e),
                }
            });
            Ok(Response::new(Body::empty()))
        } else {
            let connector = SocksConnector {
                // TODO: Can we remove this clone ?
                auth: auth.clone(),
                proxy_addr,
                connector,
            };
            let client = hyper::Client::builder().build(connector);
            let response = client.request(req).await;
            Ok(response.expect("Cannot make HTTP request"))
        }
    } else {
        let mut resp = Response::new("CONNECT must be to a socket address".into());
        *resp.status_mut() = http::StatusCode::BAD_REQUEST;
        Ok(resp)
    }
}

async fn tunnel<'t, P, T>(
    mut upgraded: Upgraded,
    plain: T,
    socks_address: P,
    auth: &Option<Auth>,
) -> Result<()>
where
    P: ToProxyAddrs,
    T: IntoTargetAddr<'t>,
{
    let mut stream = if let Some(auth) = auth {
        let username = &auth.username;
        let password = &auth.password;
        Socks5Stream::connect_with_password(socks_address, plain, username, password).await?
    } else {
        Socks5Stream::connect(socks_address, plain).await?
    };

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut stream).await?;

    // Print message when done
    debug!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );
    Ok(())
}
