# socks-to-http ![Rust](https://github.com/KaranGauswami/socks-to-http/workflows/Rust/badge.svg) ![release](https://img.shields.io/github/v/release/KaranGauswami/socks-to-http?include_prereleases)

An executable to convert SOCKS5 proxy into HTTP proxy

## About

`sthp` purpose is to create HTTP proxy on top of the Socks 5 Proxy


## How it works

It uses hyper library HTTP proxy example and adds functionality to connect via Socks5


## Compiling

Follow these instructions to compile

 1. Ensure you have current version of `cargo` and [Rust](https://www.rust-lang.org) installed
 2. Clone the project `$ git clone https://github.com/KaranGauswami/socks-to-http.git && cd socks-to-http`
 3. Build the project `$ cargo build --release`
 4. Once complete, the binary will be located at `target/release/sthp`


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

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -p, --port <port>                      port where Http proxy should listen [default: 8080]
    -s, --socks-address <socks-address>    Socks5 proxy address [default: 127.0.0.1:1080]
```


## License

`sthp` is released under the terms of either the MIT or Apache 2.0 license. See the LICENSE-MIT or LICENSE-APACHE file for the details.
