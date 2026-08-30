//! HTTP/3 integration for [`ngnet-quic`]'s ngtcp2 transport.
//!
//! The endpoint must be built with
//! [`ngnet_quic::endpoint::EndpointBuilder::build_detachable`], and its driver
//! must be polled while HTTP/3 connections are in use.

mod bridge;
mod client;
mod server;

pub use bridge::{BidiStream, Ngtcp2Connection, OpenStreams, RecvStream, SendStream};
pub use client::H3Ngtcp2Connector;
pub use server::H3Ngtcp2Acceptor;

pub use ngnet_quic;
