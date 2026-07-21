use std::sync::Arc;

use dquic::prelude::Connection;
use hyper::Uri;
use hyper::body::Bytes;

use h3_util::client::H3Connector;

/// Connector for dquic based h3 connections.
///
/// This connector wraps a dquic [`Connection`] and implements the
/// [`H3Connector`] trait to provide h3 connection capabilities.
#[derive(Clone)]
pub struct H3DquicConnector {
    uri: Uri,
    server_name: String,
    connection: Arc<Connection>,
}

impl H3DquicConnector {
    /// Create a new dquic connector from an existing QUIC connection.
    ///
    /// # Arguments
    /// * `uri` - The URI to connect to
    /// * `server_name` - The server name for TLS
    /// * `connection` - The underlying dquic connection
    pub fn new(uri: Uri, server_name: String, connection: Arc<Connection>) -> Self {
        Self {
            uri,
            server_name,
            connection,
        }
    }

    /// Get the URI this connector is configured for.
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Get the server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

impl H3Connector for H3DquicConnector {
    type CONN = h3_shim::QuicConnection;
    type OS = h3_shim::conn::OpenStreams;
    type SS = h3_shim::streams::SendStream<Bytes>;
    type RS = h3_shim::streams::RecvStream;
    type BS = h3_shim::streams::BidiStream<Bytes>;

    async fn connect(&self) -> Result<Self::CONN, h3_util::Error> {
        tracing::debug!(uri = %self.uri, server_name = %self.server_name, "connecting to dquic server");
        // Create the h3-shim QuicConnection wrapper.
        let conn = h3_shim::QuicConnection::new(self.connection.clone());
        tracing::debug!("dquic connection established");
        Ok(conn)
    }
}
