//! Full Node management — start/stop `zclassicd`, JSON-RPC, bootstrap.
//!
//! This module manages an external Zclassic daemon (`zclassicd`).
//! The wallet can operate in two modes:
//! - **P2P (Light)**: connects directly to the Zclassic network as a thin client
//! - **Full Node**: manages a local `zclassicd` daemon and communicates via JSON-RPC

#[allow(dead_code)]
pub mod bootstrap;
#[allow(dead_code)]
pub mod manager;
#[allow(dead_code)]
pub mod rpc;

#[allow(unused_imports)]
pub use manager::FullNodeManager;
#[allow(unused_imports)]
pub use rpc::RpcClient;
