use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration as StdDuration,
};

use bytes::{Buf, Bytes};
use h3_util::{client::H3Connector, server::H3Acceptor};
use ngnet_quic::{
    Duration, EntropySource, OsslBackend, OsslSession, Role,
    endpoint::{Config, Endpoint, EndpointBuilder, EndpointDriver, TokioClock, TokioSocket},
};
use ngtcp2_h3::{H3Ngtcp2Acceptor, H3Ngtcp2Connector};

const H3_ALPN: &[u8] = b"h3";
const SERVER_NAME: &str = "localhost";
const RESPONSE_BODY: &[u8] = b"hello over ngtcp2";

type Driver = EndpointDriver<TokioSocket, TokioClock, OsslBackend>;

struct Credentials {
    certificate: rcgen::CertifiedKey<rcgen::KeyPair>,
    certificate_pem: String,
    key_pem: String,
}

impl Credentials {
    fn generate() -> Self {
        let certificate = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])
            .expect("generate test certificate");
        Self {
            certificate_pem: certificate.cert.pem(),
            key_pem: certificate.signing_key.serialize_pem(),
            certificate,
        }
    }
}

struct TestEntropy(u64);

impl EntropySource for TestEntropy {
    fn fill(&mut self, buffer: &mut [u8]) -> ngnet_quic::Result<()> {
        for byte in buffer {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            *byte = self.0.to_le_bytes()[0];
        }
        Ok(())
    }
}

fn entropy(seed: u64) -> impl Fn() -> TestEntropy + Send + 'static {
    let seeds = Arc::new(AtomicU64::new(seed));
    move || {
        TestEntropy(
            seeds
                .fetch_add(0x9E37_79B9, Ordering::Relaxed)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                | 1,
        )
    }
}

async fn ngtcp2_client(credentials: &Credentials) -> (Endpoint<OsslSession>, Driver) {
    let socket = TokioSocket::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind ngtcp2 client");
    let backend = OsslBackend::builder(Role::Client)
        .alpn(H3_ALPN)
        .trust_anchor_pem(credentials.certificate_pem.as_bytes())
        .use_system_trust_store(false)
        .build()
        .expect("build client TLS backend");
    EndpointBuilder::new(socket, TokioClock::new(), backend)
        .config(Config::new().handshake_timeout(Duration::from_nanos(5_000_000_000)))
        .entropy(entropy(0xC1))
        .build_detachable()
        .expect("build detachable client endpoint")
}

async fn ngtcp2_server(
    credentials: &Credentials,
) -> (Endpoint<OsslSession>, Driver, std::net::SocketAddr) {
    let socket = TokioSocket::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind ngtcp2 server");
    let address = socket.inner().local_addr().expect("server local address");
    let backend = OsslBackend::builder(Role::Server)
        .alpn(H3_ALPN)
        .certificate_chain_pem(credentials.certificate_pem.as_bytes())
        .private_key_pem(credentials.key_pem.as_bytes())
        .build()
        .expect("build server TLS backend");
    let (endpoint, driver) = EndpointBuilder::new(socket, TokioClock::new(), backend)
        .accepts(true)
        .config(Config::new().handshake_timeout(Duration::from_nanos(5_000_000_000)))
        .entropy(entropy(0x51))
        .build_detachable()
        .expect("build detachable server endpoint");
    (endpoint, driver, address)
}

fn quinn_server(credentials: &Credentials) -> (quinn::Endpoint, std::net::SocketAddr) {
    let mut crypto = quinn::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![credentials.certificate.cert.der().clone()],
            private_key(credentials),
        )
        .expect("build quinn server TLS config");
    crypto.alpn_protocols = vec![H3_ALPN.to_vec()];
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(crypto)
            .expect("build quinn QUIC server config"),
    ));
    let endpoint = quinn::Endpoint::server(
        server_config,
        "127.0.0.1:0".parse().expect("valid loopback address"),
    )
    .expect("bind quinn server");
    let address = endpoint.local_addr().expect("quinn server local address");
    (endpoint, address)
}

fn quinn_client(credentials: &Credentials) -> quinn::Endpoint {
    let mut roots = quinn::rustls::RootCertStore::empty();
    roots
        .add(credentials.certificate.cert.der().clone())
        .expect("trust test certificate");
    let mut crypto = quinn::rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![H3_ALPN.to_vec()];

    let mut endpoint =
        quinn::Endpoint::client("127.0.0.1:0".parse().expect("valid loopback address"))
            .expect("bind quinn client");
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .expect("build quinn QUIC client config"),
    )));
    endpoint
}

fn private_key(credentials: &Credentials) -> quinn::rustls::pki_types::PrivateKeyDer<'static> {
    quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
        credentials.certificate.signing_key.serialize_der().into(),
    )
}

async fn request<C, O>(connection: C)
where
    C: h3::quic::Connection<Bytes, OpenStreams = O> + Send + 'static,
    O: h3::quic::OpenStreams<Bytes> + Send + 'static,
    C::RecvStream: Send,
    C::SendStream: Send,
{
    let (mut driver, mut sender) = h3::client::new(connection).await.expect("start h3 client");
    let request = async move {
        let mut stream = sender
            .send_request(
                http::Request::get("https://localhost/")
                    .body(())
                    .expect("build request"),
            )
            .await
            .expect("send request");
        stream.finish().await.expect("finish request");
        let response = stream.recv_response().await.expect("receive response");
        assert_eq!(response.status(), http::StatusCode::OK);

        let mut body = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await.expect("receive response body") {
            while chunk.has_remaining() {
                let bytes = chunk.chunk();
                body.extend_from_slice(bytes);
                let length = bytes.len();
                chunk.advance(length);
            }
        }
        assert_eq!(body, RESPONSE_BODY);
        drop(sender);
    };
    let drive = async move {
        let _ = driver.wait_idle().await;
    };
    tokio::join!(request, drive);
}

async fn serve<C>(connection: C)
where
    C: h3::quic::Connection<Bytes> + Send + 'static,
    C::BidiStream: Send + 'static,
{
    let mut server = h3::server::Connection::new(connection)
        .await
        .expect("start h3 server");
    let resolver = server
        .accept()
        .await
        .expect("accept h3 request")
        .expect("request stream");
    let (_request, mut stream) = resolver.resolve_request().await.expect("resolve request");
    stream
        .send_response(
            http::Response::builder()
                .status(http::StatusCode::OK)
                .body(())
                .expect("build response"),
        )
        .await
        .expect("send response");
    stream
        .send_data(Bytes::from_static(RESPONSE_BODY))
        .await
        .expect("send response body");
    stream.finish().await.expect("finish response");
    let _ = server.accept().await;
}

#[tokio::test]
async fn ngtcp2_client_connects_to_loopback_server() {
    let credentials = Credentials::generate();
    let (server_endpoint, address) = quinn_server(&credentials);
    let server_task = tokio::spawn(async move {
        let incoming = server_endpoint
            .accept()
            .await
            .expect("accept QUIC connection");
        let connection = incoming.await.expect("complete QUIC handshake");
        serve(h3_quinn::Connection::new(connection)).await;
    });

    let (endpoint, endpoint_driver) = ngtcp2_client(&credentials).await;
    let endpoint_task = tokio::spawn(endpoint_driver);
    let connector = H3Ngtcp2Connector::new(endpoint, address, Some(SERVER_NAME.to_string()));
    let connection = tokio::time::timeout(StdDuration::from_secs(10), connector.connect())
        .await
        .expect("client connection must not hang")
        .expect("connect ngtcp2 client");
    request(connection).await;

    server_task.await.expect("join server");
    endpoint_task.abort();
}

#[tokio::test]
async fn ngtcp2_server_accepts_loopback_client() {
    let credentials = Credentials::generate();
    let (endpoint, endpoint_driver, address) = ngtcp2_server(&credentials).await;
    let endpoint_task = tokio::spawn(endpoint_driver);
    let mut acceptor = H3Ngtcp2Acceptor::new(endpoint);
    let server_task = tokio::spawn(async move {
        let connection = acceptor
            .accept()
            .await
            .expect("accept ngtcp2 connection")
            .expect("endpoint is open");
        serve(connection).await;
    });

    let client_endpoint = quinn_client(&credentials);
    let connection = tokio::time::timeout(
        StdDuration::from_secs(10),
        client_endpoint
            .connect(address, SERVER_NAME)
            .expect("start quinn connection"),
    )
    .await
    .expect("client connection must not hang")
    .expect("connect quinn client");
    request(h3_quinn::Connection::new(connection)).await;

    server_task.await.expect("join server");
    endpoint_task.abort();
}
