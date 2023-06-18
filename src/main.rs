use clap::{Args, Parser};
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

#[derive(Debug, Args)]
#[group()]
struct Auths {
    /// Socks5 username
    #[arg(short = 'u', long, required = false)]
    username: String,

    /// Socks5 password
    #[arg(short = 'P', long, required = false)]
    password: String,
}

#[derive(Parser, Debug)]
#[command(author, version, about,long_about=None)]
struct Cli {
    /// port where Http proxy should listen
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    #[arg(long, default_value = "0.0.0.0")]
    listen_ip: Ipv4Addr,

    #[command(flatten)]
    auth: Option<Auths>,

    /// Socks5 proxy address
    #[arg(short, long, default_value = "127.0.0.1:1080")]
    socks_address: SocketAddr,

    /// Comma-separated list of allowed domains
    #[arg(long)]
    allowed_domains: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    color_eyre::install()?;

    let args = Cli::parse();
    let socks_address = args.socks_address;
    let port = args.port;

    let auth = match args.auth {
        Some(auth) => Some(Auth::new(auth.username, auth.password)),
        None => None,
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
    let allowed_domains = match args.allowed_domains {
        Some(domains) => Some(domains.split(',').map(|d| d.trim().to_owned()).collect()),
        None => None,
    };
    let allowed_domains = &*Box::leak(Box::new(allowed_domains));
    let make_service = make_service_fn(move |_| async move {
        Ok::<_, Infallible>(service_fn(move |req| {
            proxy(req, socks_address, auth, client, allowed_domains.clone())
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
    allowed_domains: Option<Vec<String>>,
) -> Result<Response<Body>> {
    let uri = req.uri();
    let method = req.method();
    let headers = req.headers();
    let req_str = format!("{} {} {:?}", method, uri, headers);
    log::info!("Proxying request: {}", req_str);

    if let Some(plain) = host_addr(req.uri()) {
        if let Some(allowed_domains) = allowed_domains {
            let req_domain = req.uri().host().unwrap_or("").to_owned();
            if !allowed_domains
                .iter()
                .any(|domain| req_domain.ends_with(domain))
            {
                log::warn!(
                    "Access to domain {} is not allowed through the proxy.",
                    req_domain
                );
                let mut resp = Response::new(Body::from(
                    "Access to this domain is not allowed through the proxy.",
                ));
                *resp.status_mut() = http::StatusCode::FORBIDDEN;
                return Ok(resp);
            }
        }

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
