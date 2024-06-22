use clap::{Args, Parser};
use color_eyre::eyre::Result;

use sthp::proxy::auth::Auth;
use sthp::proxy_request;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use std::net::{Ipv4Addr, SocketAddr};

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

#[derive(Parser, Debug)]
#[command(author, version, about,long_about=None)]
struct Cli {
    /// port where Http proxy should listen
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    #[arg(long, default_value = "0.0.0.0")]
    listen_ip: Ipv4Addr,

    #[command(flatten)]
    auth: Option<AuthParams>,

    /// Socks5 proxy address
    #[arg(short, long, default_value = "127.0.0.1:1080")]
    socks_address: SocketAddr,

    /// Comma-separated list of allowed domains
    #[arg(long, value_delimiter = ',')]
    allowed_domains: Option<Vec<String>>,
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

    let listener = TcpListener::bind(addr).await?;
    info!("Listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::task::spawn(async move {
            if let Err(e) = proxy_request(
                stream,
                socks_addr,
                auth_details.as_ref(),
                allowed_domains.as_ref(),
            )
            .await
            {
                error!("Error proxying request: {}", e);
            }
        });
    }
}
