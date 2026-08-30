//! HTTP/3 integration for [`ngnet-quic`]'s ngtcp2 transport.
//!
//! The endpoint must be built with
//! [`ngnet_quic::endpoint::EndpointBuilder::build_detachable`], and its driver
//! must be polled while HTTP/3 connections are in use.
//!
//! This crate has no API on Windows because its underlying transport is not
//! supported there.

#[cfg(not(windows))]
mod bridge;
#[cfg(not(windows))]
mod client;
#[cfg(not(windows))]
mod server;

#[cfg(not(windows))]
pub use bridge::{BidiStream, Ngtcp2Connection, OpenStreams, RecvStream, SendStream};
#[cfg(not(windows))]
pub use client::H3Ngtcp2Connector;
#[cfg(not(windows))]
pub use server::H3Ngtcp2Acceptor;

#[cfg(not(windows))]
pub use ngnet_quic;
