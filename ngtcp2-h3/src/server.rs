use h3_util::server::H3Acceptor;
use ngnet_quic::{Session, endpoint::Endpoint};

use crate::{BidiStream, Ngtcp2Connection, OpenStreams, RecvStream, SendStream};

/// Server acceptor backed by a detachable [`ngnet_quic`] endpoint.
pub struct H3Ngtcp2Acceptor<S: Session> {
    endpoint: Endpoint<S>,
}

impl<S: Session> H3Ngtcp2Acceptor<S> {
    /// Creates an acceptor from an endpoint configured to accept connections.
    pub fn new(endpoint: Endpoint<S>) -> Self {
        Self { endpoint }
    }

    /// Returns the underlying endpoint handle.
    pub fn endpoint(&self) -> &Endpoint<S> {
        &self.endpoint
    }
}

impl<S: Session> Clone for H3Ngtcp2Acceptor<S> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
        }
    }
}

impl<S: Session> H3Acceptor for H3Ngtcp2Acceptor<S> {
    type CONN = Ngtcp2Connection<S>;
    type OS = OpenStreams<S>;
    type SS = SendStream<S>;
    type RS = RecvStream<S>;
    type BS = BidiStream<S>;

    async fn accept(&mut self) -> Result<Option<Self::CONN>, h3_util::Error> {
        let detached = self
            .endpoint
            .accept_detached()
            .await
            .map_err(|error| Box::new(error) as h3_util::Error)?;
        Ok(Some(Ngtcp2Connection::server(detached)))
    }
}
