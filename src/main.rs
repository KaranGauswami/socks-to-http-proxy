use clap::{Args, Parser};
use color_eyre::eyre::{OptionExt, Result};

use sthp::proxy::auth::Auth;
use sthp::proxy_request;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

use base64::engine::general_purpose;
use base64::Engine;
use hyper::header::HeaderValue;
use tokio::net::TcpListener;

#[derive(Debug, Args)]
#[group()]
struct AuthParams {
    /// Socks5 username
    #[arg(short = 'u', long, required = false)]
    username: String,

    /// Socks5 password
    #[arg(short = 'P', long, required = false)]
    password: String,
}

fn socket_addr(s: &str) -> Result<SocketAddr> {
    let mut address = s.to_socket_addrs()?;
    let address = address.next();
    address.ok_or_eyre("no IP address found for the hostname".to_string())
}

#[derive(Parser, Debug)]
#[command(author, version, about,long_about=None)]
struct Cli {
    /// port where Http proxy should listen
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    #[arg(long, default_value = "127.0.0.1")]
    listen_ip: IpAddr,

    #[command(flatten)]
    auth: Option<AuthParams>,

    /// Socks5 proxy address
    #[arg(short, long, default_value = "127.0.0.1:1080", value_parser=socket_addr)]
    socks_address: SocketAddr,

    /// Comma-separated list of allowed domains
    #[arg(long, value_delimiter = ',')]
    allowed_domains: Option<Vec<String>>,

    /// HTTP Basic Auth credentials in the format "user:passwd"
    #[arg(long)]
    http_basic: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sthp=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    color_eyre::install()?;

    let args = Cli::parse();

    let socks_addr = args.socks_address;
    let port = args.port;
    let auth_details = args
        .auth
        .map(|auth| Auth::new(auth.username, auth.password));
    let auth_details = &*Box::leak(Box::new(auth_details));
    let addr = SocketAddr::from((args.listen_ip, port));
    let allowed_domains = args.allowed_domains;
    let allowed_domains = &*Box::leak(Box::new(allowed_domains));
    let http_basic = args
        .http_basic
        .map(|hb| format!("Basic {}", general_purpose::STANDARD.encode(hb)))
        .map(|hb| HeaderValue::from_str(&hb))
        .transpose()?;
    let http_basic = &*Box::leak(Box::new(http_basic));

    let listener = TcpListener::bind(addr).await?;
    info!("Listening on http://{}", addr);

    loop {
        let (stream, client_addr) = listener.accept().await?;
        tokio::task::spawn(async move {
            if let Err(e) = proxy_request(
                stream,
                client_addr,
                socks_addr,
                auth_details.as_ref(),
                allowed_domains.as_ref(),
                http_basic.as_ref(),
            )
            .await
            {
                error!("Error proxying request: {}", e);
            }
        });
    }
}
