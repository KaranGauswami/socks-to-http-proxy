use color_eyre::eyre::Result;
use futures_util::future::try_join;
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response};
use std::convert::Infallible;
use std::net::{SocketAddr, ToSocketAddrs};
use structopt::StructOpt;
use tokio_socks::IntoTargetAddr;

#[derive(StructOpt, Debug)]
#[structopt(name = "sthp")]
struct Cli {
    #[structopt(short, long, default_value = "8080")]
    /// port where Http proxy should listen
    port: u16,

    /// Socks5 proxy address
    #[structopt(short, long, default_value = "127.0.0.1:1080")]
    socks_address: String,
}

#[tokio::main]
async fn main() {
    let args = Cli::from_args();
    let socks_address = args.socks_address;
    let port = args.port;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let make_service = make_service_fn(move |_| {
        let socks_address = socks_address.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let socks_address = socks_address.clone();
                proxy(req, socks_address)
            }))
        }
    });
    let server = Server::bind(&addr).serve(make_service);
    println!("Server is listening on {}", addr);
    if let Err(e) = server.await {
        eprintln!("{:?}", e);
    };
}
async fn proxy(req: Request<Body>, socks_address: String) -> Result<Response<Body>> {
    let _response = Response::new(Body::empty());

    if req.method() == hyper::Method::CONNECT {
        tokio::task::spawn(async move {
            let plain = req.uri().authority().unwrap().as_str().to_string();
            let addr = req
                .uri()
                .authority()
                .unwrap()
                .as_str()
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap();
            match req.into_body().on_upgrade().await {
                Ok(upgraded) => {
                    if let Err(e) = tunnel(upgraded, addr, plain, socks_address).await {
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

async fn tunnel(
    upgraded: hyper::upgrade::Upgraded,
    addr: SocketAddr,
    plain: String,
    socks_address: String,
) -> std::io::Result<()> {
    let _server = tokio::net::TcpStream::connect(addr).await?;

    let socket_address = socks_address.to_socket_addrs().unwrap().next().unwrap();

    let c = plain.into_target_addr();
    let b = c.unwrap();
    let a = tokio_socks::tcp::Socks5Stream::connect(socket_address, b)
        .await
        .expect("Cannot Connect to Socks5 Server");

    let amounts = {
        let (mut server_rd, mut server_wr) = tokio::io::split(a);
        let (mut client_rd, mut client_wr) = tokio::io::split(upgraded);

        let client_to_server = tokio::io::copy(&mut client_rd, &mut server_wr);
        let server_to_client = tokio::io::copy(&mut server_rd, &mut client_wr);

        try_join(client_to_server, server_to_client).await
    };

    // Print message when done
    match amounts {
        Ok((from_client, from_server)) => {
            println!(
                "client wrote {} bytes and received {} bytes",
                from_client, from_server
            );
        }
        Err(e) => {
            eprintln!("tunnel error: {}", e);
        }
    };
    Ok(())
}
