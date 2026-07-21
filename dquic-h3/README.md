# dquic-h3

[dquic] backend for [`h3-util`], enabling HTTP/3 (and therefore
[`tonic-h3`] / [`axum-h3`]) to run over the [dquic] QUIC implementation.

This crate implements the [`h3_util::client::H3Connector`] and
[`h3_util::server::H3Acceptor`] traits on top of the [h3-shim] bridge:

- [`H3DquicConnector`] — client-side connector wrapping a dquic connection.
- [`H3DquicAcceptor`] — server-side acceptor wrapping dquic listeners.

## Status

Experimental. dquic uses SNI-based virtual hosting, so some interop
scenarios (notably with the msquic backend) have known limitations. See the
integration tests in `dquic-h3-tests` for verified working combinations.

## License

MIT

[dquic]: https://github.com/genmeta/dquic
[h3-shim]: https://crates.io/crates/h3-shim
[`h3-util`]: https://crates.io/crates/h3-util
[`tonic-h3`]: https://github.com/youyuanwu/tonic-h3
[`axum-h3`]: https://crates.io/crates/axum-h3
[`H3DquicConnector`]: https://docs.rs/dquic-h3
[`H3DquicAcceptor`]: https://docs.rs/dquic-h3
[`h3_util::client::H3Connector`]: https://docs.rs/h3-util
[`h3_util::server::H3Acceptor`]: https://docs.rs/h3-util
