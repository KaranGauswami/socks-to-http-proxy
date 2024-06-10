mod auth;

use crate::auth::Auth;
use clap::{Args, Parser};
use color_eyre::eyre::Result;

use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

use std::net::{Ipv4Addr, SocketAddr};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response};
use hyper::header::{HeaderValue, PROXY_AUTHENTICATE};

use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

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
    #[arg(long, value_delimiter = ',')]
    allowed_domains: Option<Vec<String>>,

    /// HTTP Basic Auth in the format "user:passwd"
    #[arg(long, required = false)]
    httpbasic: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sthp=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    color_eyre::install()?;

    let args = Cli::parse();

    let socks_addr = args.socks_address;
    let port = args.port;
    let auth = args
        .auth
        .map(|auth| Auth::new(auth.username, auth.password));
    let auth = &*Box::leak(Box::new(auth));
    let addr = SocketAddr::from((args.listen_ip, port));
    let allowed_domains = args.allowed_domains;
    let allowed_domains = &*Box::leak(Box::new(allowed_domains));
    let httpbasic = args.httpbasic.map(|hb| format!("Basic {}", base64::encode(hb)));
    let httpbasic = &*Box::leak(Box::new(httpbasic));

    let listener = TcpListener::bind(addr).await?;
    info!("Listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::task::spawn(proxy(
            stream,
            socks_addr,
            auth,
            allowed_domains,
            httpbasic,
        ));
    }
}

async fn proxy(
    stream: tokio::net::TcpStream,
    socks_addr: SocketAddr,
    auth: &Option<Auth>,
    allowed_domains: &Option<Vec<String>>,
    httpbasic: &Option<String>,
) -> Result<()> {
    let service = service_fn(move |req| {
        let uri = req.uri();
        // hack for HTTP Basic Auth
        let hm = req.headers();
        let mut authed = false;

        if let Some(config_auth) = httpbasic {
            if hm.contains_key("proxy-authorization") {
                let http_auth = hm.get("proxy-authorization").unwrap();
                if http_auth == config_auth {
                    authed = true;
                }
            } else {
                // 当请求没有包含Proxy-Authorization头部时，发送407响应码和Proxy-Authenticate头部
                let mut response = Response::new(full("Proxy authentication required"));
                *response.status_mut() = http::StatusCode::PROXY_AUTHENTICATION_REQUIRED;
                response.headers_mut().insert(
                    PROXY_AUTHENTICATE,
                    HeaderValue::from_static("Basic realm=\"proxy\""),
                );
                return Box::pin(async move { Ok(response) });
            }

            if !authed {
                warn!("Failed to authenticate: {:?}", hm);
                let mut resp = Response::new(full(
                    "Authorization failed, you are not allowed through the proxy.",
                ));
                *resp.status_mut() = http::StatusCode::FORBIDDEN;
                return Box::pin(async move { Ok(resp) });
            }
        }

        let method = req.method();
        debug!("Proxying request: {} {}", method, uri);
        if let (Some(allowed_domains), Some(request_domain)) = (allowed_domains, req.uri().host()) {
            if !allowed_domains.contains(&request_domain.to_string()) {
                warn!("Domain not allowed: {}", request_domain);
                return Box::pin(async move {
                    Ok(Response::builder().status(403).body(empty()).unwrap())
                });
            }
        }

        let host = match host_addr(req.uri()) {
            Some(host) => host,
            None => return Box::pin(async move {
                Ok(Response::builder().status(400).body(empty()).unwrap())
            }),
        };

        let addr = format!("{}:{}", host, req.uri().port_u16().unwrap_or(80));
        debug!("Proxying request to {} via SOCKS5 proxy at {}", addr, socks_addr);

        let stream = match auth {
            Some(auth) => Socks5Stream::connect_with_password(socks_addr, addr, &auth.username, &auth.password)
                .await
                .unwrap(),
            None => Socks5Stream::connect(socks_addr, addr).await.unwrap(),
        };

        let io = TokioIo::new(stream);

        let (mut sender, conn) = Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .handshake(io)
            .await?;
        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                warn!("Connection failed: {:?}", err);
            }
        });

        let resp = sender.send_request(req).await?;
        Ok(resp.map(|b| b.boxed()))
    });

    http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(stream, service)
        .await?;
    Ok(())
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

async fn tunnel(
    upgraded: Upgraded,
    addr: String,
    socks_addr: SocketAddr,
    auth: &Option<Auth>,
) -> Result<()> {
    let mut stream = match auth {
        Some(auth) => {
            Socks5Stream::connect_with_password(socks_addr, addr, &auth.username, &auth.password)
                .await?
        }
        None => Socks5Stream::connect(socks_addr, addr).await?,
    };

    let mut upgraded = TokioIo::new(upgraded);

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
