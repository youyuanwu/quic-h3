# quic-h3

[![build](https://github.com/youyuanwu/quic-h3/actions/workflows/build.yaml/badge.svg)](https://github.com/youyuanwu/quic-h3/actions/workflows/build.yaml)

Additional QUIC backends for [`h3-util`], the HTTP/3 abstraction layer used by
[`tonic-h3`] and [`axum-h3`].

The core backends (`quinn`, `msquic`, `s2n-quic`, `quiche`) live in the
[tonic-h3] repository. This repository hosts extra, experimental backends that
implement the same [`h3_util::client::H3Connector`] / [`h3_util::server::H3Acceptor`]
traits.

## Crates

| Crate | Backend | Status |
|-------|---------|--------|
| [`gm-quic-h3`](./gm-quic-h3) | [gm-quic] via [h3-shim] | experimental |

## Testing

Integration tests live in `gm-quic-h3-tests` and exercise the backends against a
`tonic` gRPC service, including interop with the `quinn` backend from `tonic-h3`.
The tests require `protoc` to be installed.

```sh
cargo test --all-targets
```

## License

MIT

[gm-quic]: https://github.com/genmeta/gm-quic
[h3-shim]: https://crates.io/crates/h3-shim
[`h3-util`]: https://crates.io/crates/h3-util
[`h3_util::client::H3Connector`]: https://docs.rs/h3-util
[`h3_util::server::H3Acceptor`]: https://docs.rs/h3-util
[`tonic-h3`]: https://github.com/youyuanwu/tonic-h3
[tonic-h3]: https://github.com/youyuanwu/tonic-h3
[`axum-h3`]: https://crates.io/crates/axum-h3
