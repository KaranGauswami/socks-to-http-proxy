use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use hyper::upgrade::Upgraded;
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::net::SocketAddr;
use structopt::StructOpt;
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::IntoTargetAddr;

#[derive(StructOpt, Debug)]
#[structopt(name = "sthp")]
struct Cli {
    #[structopt(short, long, default_value = "8080")]
    /// port where Http proxy should listen
    port: u16,

    /// Socks5 proxy address
    #[structopt(short, long, default_value = "127.0.0.1:1080")]
    socks_address: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::from_args();
    let socks_address = args.socks_address;
    let port = args.port;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let make_service = make_service_fn(move |_| async move {
        Ok::<_, Infallible>(service_fn(move |req| proxy(req, socks_address.clone())))
    });
    let server = Server::bind(&addr)
        .http1_preserve_header_case(true)
        .http1_title_case_headers(true)
        .serve(make_service);
    println!("Server is listening on {}", addr);
    if let Err(e) = server.await {
        eprintln!("{:?}", e);
    };
    Ok(())
}
async fn proxy(req: Request<Body>, socks_address: SocketAddr) -> Result<Response<Body>> {
    if req.method() == hyper::Method::CONNECT {
        tokio::task::spawn(async move {
            let plain = req.uri().authority().unwrap().as_str().to_string();
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = tunnel(upgraded, plain, socks_address).await {
                        eprintln!("server io error: {}", e);
                    };
                }
                Err(e) => eprintln!("upgrade error: {}", e),
            }
        });
        Ok(Response::new(Body::empty()))
    } else {
        Ok(Response::new(Body::empty()))
    }
}

async fn tunnel<'t, I>(mut upgraded: Upgraded, plain: I, socks_address: SocketAddr) -> Result<()>
where
    I: IntoTargetAddr<'t>,
{
    let mut stream = Socks5Stream::connect(socks_address, plain).await?;

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut stream).await?;

    // Print message when done
    println!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );
    Ok(())
}
