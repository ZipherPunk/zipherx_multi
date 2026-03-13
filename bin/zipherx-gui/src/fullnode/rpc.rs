//! JSON-RPC client for communicating with `zclassicd`.
//!
//! Uses synchronous HTTP (ureq) since RPC calls run on the background thread.

use serde_json::Value;

/// JSON-RPC client for the Zclassic daemon.
pub struct RpcClient {
    url: String,
    user: String,
    password: String,
}

/// Parsed RPC response.
#[derive(Debug)]
pub struct RpcResponse {
    pub result: Value,
    #[allow(dead_code)]
    pub error: Option<RpcError>,
}

/// RPC error from daemon.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcClient {
    pub fn new(url: &str, user: &str, password: &str) -> Self {
        Self {
            url: url.to_string(),
            user: user.to_string(),
            password: password.to_string(),
        }
    }

    /// Call an RPC method with the given parameters.
    ///
    // SECURITY (GUI-M1): Basic Auth over localhost TCP is acceptable per HTTP spec.
    // The RPC port (8023) is bound to 127.0.0.1 by zclassicd default config.
    // No network traffic leaves the machine.
    pub fn call(&self, method: &str, params: &[Value]) -> Result<Value, String> {
        let body = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "zipherx",
            "method": method,
            "params": params
        });

        let response = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .set(
                "Authorization",
                &format!(
                    "Basic {}",
                    base64_encode(&format!("{}:{}", self.user, self.password))
                ),
            )
            .timeout(std::time::Duration::from_secs(30))
            .send_string(&body.to_string())
            .map_err(|e| format!("RPC connection failed: {}", e))?;

        let json: Value = response
            .into_json()
            .map_err(|e| format!("RPC response parse failed: {}", e))?;

        if let Some(error) = json.get("error") {
            if !error.is_null() {
                let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                // GUI-L4: Map RPC error codes to user-friendly messages
                // to avoid leaking internal daemon details.
                let user_msg = match code {
                    -28 => "Daemon is still loading (warming up)".to_string(),
                    -1 => "RPC command error".to_string(),
                    _ => format!("RPC error (code {})", code),
                };
                return Err(user_msg);
            }
        }

        json.get("result")
            .cloned()
            .ok_or_else(|| "RPC response missing 'result' field".to_string())
    }

    /// `getinfo` — general node information.
    pub fn get_info(&self) -> Result<Value, String> {
        self.call("getinfo", &[])
    }

    /// `getblockchaininfo` — blockchain sync status.
    pub fn get_blockchain_info(&self) -> Result<super::manager::ChainInfo, String> {
        let result = self.call("getblockchaininfo", &[])?;
        Ok(super::manager::ChainInfo {
            blocks: result.get("blocks").and_then(|v| v.as_u64()).unwrap_or(0),
            headers: result.get("headers").and_then(|v| v.as_u64()).unwrap_or(0),
            verification_progress: result
                .get("verificationprogress")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            size_on_disk: result
                .get("size_on_disk")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            pruned: result
                .get("pruned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            chain: result
                .get("chain")
                .and_then(|v| v.as_str())
                .unwrap_or("main")
                .to_string(),
        })
    }

    /// `getnetworkinfo` — network status.
    pub fn get_network_info(&self) -> Result<super::manager::NetworkInfo, String> {
        let result = self.call("getnetworkinfo", &[])?;
        Ok(super::manager::NetworkInfo {
            version: result.get("version").and_then(|v| v.as_u64()).unwrap_or(0),
            subversion: result
                .get("subversion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            protocol_version: result
                .get("protocolversion")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            connections: result
                .get("connections")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        })
    }

    /// `getpeerinfo` — connected peer details.
    pub fn get_peer_info(&self) -> Result<Vec<Value>, String> {
        let result = self.call("getpeerinfo", &[])?;
        result
            .as_array()
            .cloned()
            .ok_or_else(|| "getpeerinfo: expected array".to_string())
    }

    /// `getmininginfo` — mining status.
    pub fn get_mining_info(&self) -> Result<Value, String> {
        self.call("getmininginfo", &[])
    }

    /// `getmempoolinfo` — mempool status.
    pub fn get_mempool_info(&self) -> Result<Value, String> {
        self.call("getmempoolinfo", &[])
    }

    /// `stop` — request graceful daemon shutdown.
    pub fn stop(&self) -> Result<Value, String> {
        self.call("stop", &[])
    }

    /// Test if the RPC connection is alive.
    ///
    // GUI-L5: is_alive() uses the default 30s HTTP timeout. For a health check,
    // a shorter timeout would be preferable but ureq doesn't support per-request
    // timeouts without creating a new agent. The 5s polling interval in the UI
    // prevents multiple overlapping health checks.
    pub fn is_alive(&self) -> bool {
        self.get_info().is_ok()
    }
}

/// Simple base64 encoding for Basic auth.
fn base64_encode(input: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}
