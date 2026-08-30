use std::net::SocketAddr;

use bytes::Bytes;
use h3_util::client::H3Connector;
use ngnet_quic::{Session, endpoint::Endpoint};

use crate::{BidiStream, Ngtcp2Connection, OpenStreams, RecvStream, SendStream};

/// Client connector backed by a detachable [`ngnet_quic`] endpoint.
pub struct H3Ngtcp2Connector<S: Session> {
    endpoint: Endpoint<S>,
    remote: SocketAddr,
    server_name: Option<String>,
}

impl<S: Session> H3Ngtcp2Connector<S> {
    /// Creates a connector for `remote`.
    pub fn new(
        endpoint: Endpoint<S>,
        remote: SocketAddr,
        server_name: Option<String>,
    ) -> Self {
        Self {
            endpoint,
            remote,
            server_name,
        }
    }

    /// Returns the remote address.
    pub fn remote_address(&self) -> SocketAddr {
        self.remote
    }

    /// Returns the TLS server name, if configured.
    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }
}

impl<S: Session> Clone for H3Ngtcp2Connector<S> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            remote: self.remote,
            server_name: self.server_name.clone(),
        }
    }
}

impl<S: Session> H3Connector for H3Ngtcp2Connector<S> {
    type CONN = Ngtcp2Connection<S>;
    type OS = OpenStreams<S>;
    type SS = SendStream<S>;
    type RS = RecvStream<S>;
    type BS = BidiStream<S>;

    async fn connect(&self) -> Result<Self::CONN, h3_util::Error> {
        let detached = self
            .endpoint
            .connect_detached(self.remote, self.server_name.as_deref())
            .await
            .map_err(|error| Box::new(error) as h3_util::Error)?;
        Ok(Ngtcp2Connection::client(detached))
    }
}

const _: fn() = || {
    fn check<S: Session>()
    where
        Ngtcp2Connection<S>: h3::quic::Connection<Bytes>,
    {
    }
};
