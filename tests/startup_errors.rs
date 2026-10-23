use std::net::TcpListener;
use std::process::Command;

#[test]
fn occupied_listen_port_returns_error_without_panicking() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
    let port = listener
        .local_addr()
        .expect("get test listener address")
        .port();

    let output = Command::new(env!("CARGO_BIN_EXE_sthp"))
        .args(["--listen-ip", "127.0.0.1", "--port", &port.to_string()])
        .env_remove("SOCKS5_USER")
        .env_remove("SOCKS5_PASSWORD")
        .output()
        .expect("run sthp");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("failed to bind HTTP proxy to 127.0.0.1:{port}")),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("panicked at"), "stderr: {stderr}");
}
