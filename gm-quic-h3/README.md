# gm-quic-h3

[gm-quic] backend for [`h3-util`], enabling HTTP/3 (and therefore
[`tonic-h3`] / [`axum-h3`]) to run over the [gm-quic] QUIC implementation.

This crate implements the [`h3_util::client::H3Connector`] and
[`h3_util::server::H3Acceptor`] traits on top of the [h3-shim] bridge:

- [`H3GmQuicConnector`] — client-side connector wrapping a gm-quic connection.
- [`H3GmQuicAcceptor`] — server-side acceptor wrapping gm-quic listeners.

## Status

Experimental. gm-quic uses SNI-based virtual hosting, so some interop
scenarios (notably with the msquic backend) have known limitations. See the
integration tests in `gm-quic-h3-tests` for verified working combinations.

## License

MIT

[gm-quic]: https://github.com/genmeta/gm-quic
[h3-shim]: https://crates.io/crates/h3-shim
[`h3-util`]: https://crates.io/crates/h3-util
[`tonic-h3`]: https://github.com/youyuanwu/tonic-h3
[`axum-h3`]: https://crates.io/crates/axum-h3
[`H3GmQuicConnector`]: https://docs.rs/gm-quic-h3
[`H3GmQuicAcceptor`]: https://docs.rs/gm-quic-h3
[`h3_util::client::H3Connector`]: https://docs.rs/h3-util
[`h3_util::server::H3Acceptor`]: https://docs.rs/h3-util
