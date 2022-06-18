# socks-to-http-proxy ![Rust](https://github.com/KaranGauswami/socks-to-http-proxy/workflows/Rust/badge.svg) ![release](https://img.shields.io/github/v/release/KaranGauswami/socks-to-http-proxy?include_prereleases)

An executable to convert SOCKS5 proxy into HTTP proxy

## About

`sthp` purpose is to create HTTP proxy on top of the Socks 5 Proxy

## How it works

It uses hyper library HTTP proxy example and adds functionality to connect via Socks5

## Compiling

Follow these instructions to compile

1.  Ensure you have current version of `cargo` and [Rust](https://www.rust-lang.org) installed
2.  Clone the project `$ git clone https://github.com/KaranGauswami/socks-to-http-proxy.git && cd socks-to-http-proxy`
3.  Build the project `$ cargo build --release`
4.  Once complete, the binary will be located at `target/release/sthp`

## Usage

```bash
sthp -p 8080 -s 127.0.0.1:1080
```

This will create proxy server on 8080 and use localhost:1080 as a Socks5 Proxy

### Options

There are a few options for using `sthp`.

```text
USAGE:
    sthp [OPTIONS]

OPTIONS:
    -h, --help                             Print help information
        --listen-ip <LISTEN_IP>            [default: 0.0.0.0]
    -p, --port <PORT>                      port where Http proxy should listen [default: 8080]
    -P, --password <PASSWORD>              Socks5 password
    -s, --socks-address <SOCKS_ADDRESS>    Socks5 proxy address [default: 127.0.0.1:1080]
    -u, --username <USERNAME>              Socks5 username
    -V, --version                          Print version information    
```
