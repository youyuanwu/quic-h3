use std::sync::Arc;

use dquic::prelude::QuicListeners;
use hyper::body::Bytes;

use h3_util::server::H3Acceptor;

/// Acceptor for dquic based h3 connections.
///
/// This acceptor wraps a dquic [`QuicListeners`] and implements the
/// [`H3Acceptor`] trait to accept incoming h3 connections.
#[derive(Clone)]
pub struct H3DquicAcceptor {
    listeners: Arc<QuicListeners>,
}

impl H3DquicAcceptor {
    /// Create a new dquic acceptor from a [`QuicListeners`] instance.
    ///
    /// # Arguments
    /// * `listeners` - The dquic listeners that will accept incoming connections
    pub fn new(listeners: Arc<QuicListeners>) -> Self {
        Self { listeners }
    }

    /// Get a reference to the underlying listeners.
    pub fn listeners(&self) -> &QuicListeners {
        &self.listeners
    }

    /// Shutdown the acceptor and close all listeners.
    /// This consumes the acceptor to ensure proper cleanup.
    pub async fn shutdown(self) {
        // Explicitly shutdown the listeners.
        self.listeners.shutdown();
        // Wait a bit for cleanup.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

impl H3Acceptor for H3DquicAcceptor {
    type CONN = h3_shim::QuicConnection;
    type OS = h3_shim::conn::OpenStreams;
    type SS = h3_shim::streams::SendStream<Bytes>;
    type RS = h3_shim::streams::RecvStream;
    type BS = h3_shim::streams::BidiStream<Bytes>;

    async fn accept(&mut self) -> Result<Option<Self::CONN>, h3_util::Error> {
        match self.listeners.accept().await {
            Ok((new_conn, _server, _pathway, _link)) => {
                tracing::debug!("dquic accepted new connection");
                let conn = h3_shim::QuicConnection::new(new_conn);
                Ok(Some(conn))
            }
            Err(e) => {
                tracing::debug!("dquic accept error: {:?}", e);
                Err(format!("dquic accept error: {:?}", e).into())
            }
        }
    }
}
