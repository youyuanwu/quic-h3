# ngtcp2-h3

An [`ngnet-quic`] backend for [`h3-util`], enabling HTTP/3 users such as
[`tonic-h3`] and [`axum-h3`] to run over ngtcp2.

The crate implements the `h3::quic` transport traits and provides:

- `H3Ngtcp2Connector` for outbound connections.
- `H3Ngtcp2Acceptor` for inbound connections.
- `Ngtcp2Connection` for callers that already have a detached connection.

Build the underlying endpoint with
`EndpointBuilder::build_detachable` and keep its driver running for the lifetime
of every HTTP/3 connection.

The native `ngnet-quic` dependencies require a C compiler, CMake, and libclang.

## License

MIT

[`ngnet-quic`]: https://crates.io/crates/ngnet-quic
[`h3-util`]: https://crates.io/crates/h3-util
[`tonic-h3`]: https://github.com/youyuanwu/tonic-h3
[`axum-h3`]: https://crates.io/crates/axum-h3
