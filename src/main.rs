use clap::{ErrorKind, IntoApp, Parser};
use color_eyre::eyre::Result;
use http::Uri;
use hyper::client::HttpConnector;
use hyper::service::{make_service_fn, service_fn};
use hyper::upgrade::Upgraded;
use hyper::{Body, Client, Request, Response, Server};
use hyper_socks2::{Auth, SocksConnector};
use log::debug;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::{IntoTargetAddr, ToProxyAddrs};

#[derive(Parser, Debug)]
#[clap(author, version, about,long_about=None)]
struct Cli {
    /// port where Http proxy should listen
    #[clap(short, long, default_value = "8080",value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    #[clap(long, default_value = "0.0.0.0")]
    listen_ip: Ipv4Addr,

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
    color_eyre::install()?;

    let args = Cli::parse();
    let socks_address = args.socks_address;
    let port = args.port;

    let username = args.username;
    let password = args.password;
    let mut cmd = Cli::command();

    let auth = match (username, password) {
        (Some(username), Some(password)) => Some(Auth::new(username, password)),
        (Some(_), None) => cmd
            .error(
                ErrorKind::ArgumentNotFound,
                "--password is required if --username is used",
            )
            .exit(),
        (None, Some(_)) => cmd
            .error(
                ErrorKind::ArgumentNotFound,
                "--username is required if --password is used",
            )
            .exit(),
        (None, None) => None,
    };
    let auth = &*Box::leak(Box::new(auth));
    let addr = SocketAddr::from((args.listen_ip, port));
    let mut connector = HttpConnector::new();
    connector.enforce_http(false);
    let proxy_addr = format!("socks://{socks_address}").parse::<Uri>()?;
    let connector = SocksConnector {
        auth: auth.clone(),
        proxy_addr,
        connector,
    };
    let client: Client<SocksConnector<HttpConnector>> = hyper::Client::builder().build(connector);
    let client = &*Box::leak(Box::new(client));
    let make_service = make_service_fn(move |_| async move {
        Ok::<_, Infallible>(service_fn(move |req| {
            proxy(req, socks_address, auth, client)
        }))
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
    client: &'static Client<SocksConnector<HttpConnector>>,
) -> Result<Response<Body>> {
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
