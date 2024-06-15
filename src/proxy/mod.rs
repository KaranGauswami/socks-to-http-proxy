use auth::Auth;
use color_eyre::eyre::Result;

pub mod auth;

use hyper::service::service_fn;
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use tracing::{debug, warn};

use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response};

use hyper_util::rt::TokioIo;

use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;

async fn proxy(
    req: Request<hyper::body::Incoming>,
    socks_addr: SocketAddr,
    auth: &'static Option<Auth>,
    allowed_domains: &Option<Vec<String>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let uri = req.uri();
    let method = req.method();
    debug!("Proxying request: {} {}", method, uri);
    if let (Some(allowed_domains), Some(request_domain)) = (allowed_domains, req.uri().host()) {
        let domain = request_domain.to_owned();
        if !allowed_domains.contains(&domain) {
            warn!(
                "Access to domain {} is not allowed through the proxy.",
                domain
            );
            let mut resp = Response::new(full(
                "Access to this domain is not allowed through the proxy.",
            ));
            *resp.status_mut() = http::StatusCode::FORBIDDEN;
            return Ok(resp);
        }
    }

    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr, socks_addr, auth).await {
                            warn!("server io error: {}", e);
                        };
                    }
                    Err(e) => warn!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(empty()))
        } else {
            warn!("CONNECT host is not socket addr: {:?}", req.uri());
            let mut resp = Response::new(full("CONNECT must be to a socket address"));
            *resp.status_mut() = http::StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        let host = req.uri().host().expect("uri has no host");
        let port = req.uri().port_u16().unwrap_or(80);
        let addr = format!("{}:{}", host, port);

        let stream = match auth {
            Some(auth) => Socks5Stream::connect_with_password(
                socks_addr,
                addr,
                &auth.username,
                &auth.password,
            )
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
    }
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

pub async fn proxy_request(
    stream: TcpStream,
    socks_addr: SocketAddr,
    auth_details: &'static Option<Auth>,
    allowed_domains: &'static Option<Vec<String>>,
) {
    let io = TokioIo::new(stream);

    let serve_connection =
        service_fn(move |req| proxy(req, socks_addr, auth_details, allowed_domains));

    tokio::task::spawn(async move {
        if let Err(err) = http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .serve_connection(io, serve_connection)
            .with_upgrades()
            .await
        {
            warn!("Failed to serve connection: {:?}", err);
        }
    });
}
