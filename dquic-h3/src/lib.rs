//! dquic backend for [`h3-util`].
//!
//! This crate provides integration with the [dquic] QUIC implementation
//! through the [h3-shim] crate. It implements the [`h3_util::client::H3Connector`]
//! and [`h3_util::server::H3Acceptor`] traits so that the HTTP/3 client and
//! server machinery in [`h3-util`] (and therefore [`tonic-h3`] / [`axum-h3`])
//! can run over dquic.
//!
//! # Backends
//!
//! - [`H3DquicConnector`] — client-side connector wrapping a dquic connection.
//! - [`H3DquicAcceptor`] — server-side acceptor wrapping dquic listeners.
//!
//! # Example
//!
//! ## Client
//! ```ignore
//! use dquic_h3::{H3DquicConnector, dquic::prelude::*};
//! use std::sync::Arc;
//!
//! // Create a dquic client.
//! let client = QuicClient::builder()
//!     .without_verifier()
//!     .without_cert()
//!     .with_alpns(["h3"])
//!     .build();
//!
//! // Connect to a server.
//! let connection = client.connect("example.com:443").await?;
//!
//! // Create the H3 connector.
//! let connector = H3DquicConnector::new(
//!     "https://example.com".parse()?,
//!     "example.com".to_string(),
//!     Arc::new(connection),
//! );
//! ```
//!
//! ## Server
//! ```ignore
//! use dquic_h3::{H3DquicAcceptor, dquic::prelude::*};
//! use std::sync::Arc;
//!
//! // Create dquic listeners.
//! let listeners = QuicListeners::builder()?
//!     .without_client_cert_verifier()
//!     .with_alpns(["h3"])
//!     .listen(8);
//! listeners.add_server("example.com", "cert.pem", "key.pem", ["0.0.0.0:443"], None::<Vec<u8>>)?;
//!
//! // Create the H3 acceptor.
//! let mut acceptor = H3DquicAcceptor::new(listeners);
//!
//! // Accept incoming connections.
//! while let Ok(Some(conn)) = acceptor.accept().await {
//!     // Handle connection.
//! }
//! ```
//!
//! [dquic]: https://github.com/genmeta/dquic
//! [h3-shim]: https://crates.io/crates/h3-shim
//! [`h3-util`]: https://crates.io/crates/h3-util
//! [`tonic-h3`]: https://github.com/youyuanwu/tonic-h3
//! [`axum-h3`]: https://crates.io/crates/axum-h3

pub mod client;
pub mod server;

pub use client::H3DquicConnector;
pub use server::H3DquicAcceptor;

// Re-export the underlying crates for convenience.
pub use dquic;
pub use h3_shim;
