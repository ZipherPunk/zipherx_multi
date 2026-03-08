//! SOCKS5 proxy connection for Tor routing.
//!
//! Connects a TcpStream through a SOCKS5 proxy (Arti Tor client) to reach
//! .onion peers or route clearnet peers through Tor for privacy.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

use crate::types::NetworkError;

/// SOCKS5 protocol constants.
const SOCKS5_VERSION: u8 = 0x05;
const AUTH_NO_AUTH: u8 = 0x00;
const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const REPLY_SUCCEEDED: u8 = 0x00;

/// Connect to a target host through a SOCKS5 proxy.
///
/// The semaphore limits concurrent SOCKS5 handshakes (default: 6)
/// to avoid saturating the Tor proxy.
pub async fn connect_via_socks5(
    proxy_addr: SocketAddr,
    target_host: &str,
    target_port: u16,
    semaphore: &Semaphore,
    timeout: Duration,
) -> Result<TcpStream, NetworkError> {
    // Acquire semaphore permit (limits concurrent Tor handshakes)
    let _permit = tokio::time::timeout(timeout, semaphore.acquire())
        .await
        .map_err(|_| NetworkError::Socks5Error("Semaphore timeout".into()))?
        .map_err(|_| NetworkError::Socks5Error("Semaphore closed".into()))?;

    // Connect to SOCKS5 proxy
    let mut stream = tokio::time::timeout(timeout, TcpStream::connect(proxy_addr))
        .await
        .map_err(|_| NetworkError::Socks5Error("Proxy connection timeout".into()))?
        .map_err(|e| NetworkError::Socks5Error(format!("Proxy connect: {e}")))?;

    // Greeting: version(1) + num_methods(1) + methods(1) = [0x05, 0x01, 0x00]
    stream
        .write_all(&[SOCKS5_VERSION, 0x01, AUTH_NO_AUTH])
        .await
        .map_err(|e| NetworkError::Socks5Error(format!("Greeting write: {e}")))?;

    // Response: version(1) + chosen_method(1)
    let mut greeting_resp = [0u8; 2];
    stream
        .read_exact(&mut greeting_resp)
        .await
        .map_err(|e| NetworkError::Socks5Error(format!("Greeting read: {e}")))?;

    if greeting_resp[0] != SOCKS5_VERSION {
        return Err(NetworkError::Socks5Error(format!(
            "Invalid SOCKS version: {}",
            greeting_resp[0]
        )));
    }
    if greeting_resp[1] != AUTH_NO_AUTH {
        return Err(NetworkError::Socks5Error(format!(
            "Unsupported auth method: {}",
            greeting_resp[1]
        )));
    }

    // Connect request
    let mut request = Vec::new();
    request.push(SOCKS5_VERSION);
    request.push(CMD_CONNECT);
    request.push(0x00); // RSV

    // Determine address type
    if let Ok(ip) = target_host.parse::<std::net::Ipv4Addr>() {
        request.push(ATYP_IPV4);
        request.extend_from_slice(&ip.octets());
    } else {
        // Domain name (including .onion)
        let host_bytes = target_host.as_bytes();
        if host_bytes.len() > 255 {
            return Err(NetworkError::Socks5Error("Domain too long".into()));
        }
        request.push(ATYP_DOMAIN);
        request.push(host_bytes.len() as u8);
        request.extend_from_slice(host_bytes);
    }

    request.extend_from_slice(&target_port.to_be_bytes());

    stream
        .write_all(&request)
        .await
        .map_err(|e| NetworkError::Socks5Error(format!("Connect write: {e}")))?;

    // Response: version(1) + reply(1) + rsv(1) + atyp(1) + ...
    let mut resp_header = [0u8; 4];
    stream
        .read_exact(&mut resp_header)
        .await
        .map_err(|e| NetworkError::Socks5Error(format!("Connect read: {e}")))?;

    if resp_header[1] != REPLY_SUCCEEDED {
        return Err(NetworkError::Socks5Error(format!(
            "SOCKS5 connect failed: code {}",
            resp_header[1]
        )));
    }

    // Read bound address (skip it — we don't need it, but must consume the bytes)
    match resp_header[3] {
        ATYP_IPV4 => {
            let mut addr = [0u8; 6]; // 4 IP + 2 port
            stream.read_exact(&mut addr).await
                .map_err(|e| NetworkError::Socks5Error(format!("Read bound addr (IPv4): {e}")))?;
        }
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await
                .map_err(|e| NetworkError::Socks5Error(format!("Read bound addr domain len: {e}")))?;
            let mut addr = vec![0u8; len[0] as usize + 2];
            stream.read_exact(&mut addr).await
                .map_err(|e| NetworkError::Socks5Error(format!("Read bound addr (domain): {e}")))?;
        }
        0x04 => {
            // IPv6
            let mut addr = [0u8; 18]; // 16 IP + 2 port
            stream.read_exact(&mut addr).await
                .map_err(|e| NetworkError::Socks5Error(format!("Read bound addr (IPv6): {e}")))?;
        }
        other => {
            // NET-003: Unknown address type leaves unread bytes on the stream —
            // return an error instead of proceeding with a corrupted stream.
            return Err(NetworkError::Socks5Error(format!(
                "Unknown SOCKS5 address type: 0x{:02x}", other
            )));
        }
    }

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Mock SOCKS5 server that accepts any connection.
    async fn mock_socks5_server(listener: TcpListener) {
        let (mut stream, _) = listener.accept().await.unwrap();

        // Read greeting
        let mut greeting = [0u8; 3];
        stream.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting[0], SOCKS5_VERSION);

        // Send response: no auth required
        stream.write_all(&[SOCKS5_VERSION, AUTH_NO_AUTH]).await.unwrap();

        // Read connect request (variable length)
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], SOCKS5_VERSION);
        assert_eq!(header[1], CMD_CONNECT);

        // Read address
        match header[3] {
            ATYP_IPV4 => {
                let mut addr = [0u8; 6];
                stream.read_exact(&mut addr).await.unwrap();
            }
            ATYP_DOMAIN => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await.unwrap();
                let mut addr = vec![0u8; len[0] as usize + 2];
                stream.read_exact(&mut addr).await.unwrap();
            }
            _ => {}
        }

        // Send success response: IPv4 bound address 0.0.0.0:0
        stream
            .write_all(&[
                SOCKS5_VERSION,
                REPLY_SUCCEEDED,
                0x00,
                ATYP_IPV4,
                0, 0, 0, 0, // IP
                0, 0, // port
            ])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_socks5_connect_ipv4() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let sem = Semaphore::new(6);

        tokio::spawn(mock_socks5_server(listener));

        let stream = connect_via_socks5(
            proxy_addr,
            "1.2.3.4",
            8233,
            &sem,
            Duration::from_secs(5),
        )
        .await;

        assert!(stream.is_ok());
    }

    #[tokio::test]
    async fn test_socks5_connect_domain() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let sem = Semaphore::new(6);

        tokio::spawn(mock_socks5_server(listener));

        let stream = connect_via_socks5(
            proxy_addr,
            "example.onion",
            8233,
            &sem,
            Duration::from_secs(5),
        )
        .await;

        assert!(stream.is_ok());
    }

    #[tokio::test]
    async fn test_socks5_connect_timeout() {
        // Connect to a non-listening address
        let sem = Semaphore::new(6);
        let result = connect_via_socks5(
            "127.0.0.1:1".parse().unwrap(),
            "example.com",
            80,
            &sem,
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_socks5_semaphore_limits() {
        let sem = Semaphore::new(1);

        // Acquire the only permit
        let _permit = sem.acquire().await.unwrap();

        // Second connection should timeout on semaphore
        let result = connect_via_socks5(
            "127.0.0.1:1".parse().unwrap(),
            "example.com",
            80,
            &sem,
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_err());
    }
}
