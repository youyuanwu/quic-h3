//! Integration tests for the `dquic-h3` backend.
//!
//! These tests exercise the [`dquic_h3::H3DquicConnector`] and
//! [`dquic_h3::H3DquicAcceptor`] against a `tonic` greeter service, both on
//! their own and interoperating with the `quinn` backend from `tonic-h3`.

use std::{net::SocketAddr, sync::Arc};

use h3_util::server::H3Acceptor;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod dquic;
#[cfg(test)]
mod mix;

pub mod cert_gen;

// The string specified here must match the proto package name.
tonic::include_proto!("helloworld");

#[derive(Default)]
pub struct HelloWorldService {}

#[tonic::async_trait]
impl crate::greeter_server::Greeter for HelloWorldService {
    async fn say_hello(
        &self,
        req: tonic::Request<HelloRequest>,
    ) -> Result<tonic::Response<HelloReply>, tonic::Status> {
        let name = req.into_inner().name;
        tracing::debug!("say_hello: {}", name);
        Ok(tonic::Response::new(HelloReply {
            message: format!("hello {name}"),
        }))
    }
}

pub fn make_test_cert_rustls(
    subject_alt_names: Vec<String>,
) -> (
    rustls::pki_types::CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let (cert, key_pair) = cert_gen::make_test_cert(subject_alt_names);
    let cert = rustls::pki_types::CertificateDer::from(cert);
    use rustls::pki_types::pem::PemObject;
    let key = rustls::pki_types::PrivateKeyDer::from_pem(
        rustls::pki_types::pem::SectionKind::PrivateKey,
        key_pair.serialize_der(),
    )
    .unwrap();
    (cert, key)
}

pub fn try_setup_tracing() {
    // Install the rustls ring crypto provider for dquic compatibility
    // (dquic uses rustls 0.23+ which requires explicit provider setup).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

pub fn make_quinn_server_endpoint(in_addr: SocketAddr) -> quinn::Endpoint {
    let tls_config = Arc::new(make_rustls_server_config());

    let server_config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls_config).unwrap(),
    ));
    quinn::Endpoint::server(server_config, in_addr).unwrap()
}

pub fn run_test_server(
    acceptor: impl H3Acceptor + Send + 'static,
    token: CancellationToken,
) -> tokio::task::JoinHandle<Result<(), tonic_h3::Error>> {
    let hello_svc = crate::HelloWorldService {};
    let router = tonic::service::Routes::builder()
        .add_service(crate::greeter_server::GreeterServer::new(hello_svc))
        .clone()
        .routes();

    // run server in background
    tokio::spawn(async move {
        tonic_h3::server::H3Router::new(router)
            .serve_with_shutdown(acceptor, async move { token.cancelled().await })
            .await
    })
}

// returns handle and listening addr
pub fn run_test_quinn_hello_server(
    in_addr: SocketAddr,
    token: CancellationToken,
) -> (tokio::task::JoinHandle<()>, SocketAddr) {
    let endpoint = make_quinn_server_endpoint(in_addr);
    let listen_addr = endpoint.local_addr().unwrap();
    tracing::debug!("listenaddr : {}", listen_addr);
    let acceptor = tonic_h3::quinn::H3QuinnAcceptor::new(endpoint.clone());
    let h_sv = run_test_server(acceptor, token);

    let h = tokio::spawn(async move {
        h_sv.await
            .expect("cannot join")
            .expect("tonic server failed");
        endpoint.close(0_u16.into(), b"svr shutdown");
        endpoint.wait_idle().await;
        tracing::debug!("test server ended")
    });

    (h, listen_addr)
}

// copied from https://github.com/rustls/rustls/blob/f98484bdbd57a57bafdd459db594e21c531f1b4a/examples/src/bin/tlsclient-mio.rs#L331
mod danger {
    use rustls::DigitallySignedStruct;
    use rustls::client::danger::HandshakeSignatureValid;
    use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

    #[derive(Debug)]
    pub struct NoCertificateVerification(CryptoProvider);

    impl NoCertificateVerification {
        pub fn new(provider: CryptoProvider) -> Self {
            Self(provider)
        }
    }

    impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

pub fn make_danger_rustls_client_config() -> rustls::ClientConfig {
    let mut tls_config = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .unwrap()
    .dangerous() // Do not verify server certs
    .with_custom_certificate_verifier(Arc::new(crate::danger::NoCertificateVerification::new(
        rustls::crypto::ring::default_provider(),
    )))
    .with_no_client_auth();

    tls_config.enable_early_data = true;
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    tls_config
}

pub fn make_rustls_server_config() -> rustls::ServerConfig {
    let (cert, key) = crate::make_test_cert_rustls(vec!["localhost".to_string()]);
    let mut tls_config = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert.clone()], key.clone_key())
    .unwrap();
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    tls_config.max_early_data_size = u32::MAX;
    tls_config
}

pub fn make_test_quinn_client_endpoint() -> quinn::Endpoint {
    let tls_config = make_danger_rustls_client_config();
    let mut client_endpoint = quinn::Endpoint::client("[::]:0".parse().unwrap()).unwrap();
    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap(),
    ));
    client_endpoint.set_default_client_config(client_config);
    client_endpoint
}

pub mod dquic_util {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use dquic::prelude::{QuicClient, QuicListeners};
    use dquic_h3::H3DquicAcceptor;
    use tokio_util::sync::CancellationToken;

    pub fn make_test_dquic_client() -> Arc<QuicClient> {
        // Create a dquic client with no certificate verification (for testing).
        // Must set ALPN to "h3" for HTTP/3 protocol negotiation.
        // `QuicClient::connect` is invoked on an `Arc<QuicClient>`.
        Arc::new(
            QuicClient::builder()
                .without_verifier()
                .without_cert()
                .with_alpns(["h3"])
                .build(),
        )
    }

    pub async fn run_test_dquic_server(
        in_addr: SocketAddr,
        token: CancellationToken,
    ) -> (tokio::task::JoinHandle<()>, SocketAddr) {
        use dquic::prelude::IO;

        // Generate test certificates.
        let (cert_path, key_path) = crate::cert_gen::make_test_cert_files("dquic", false);

        // Create dquic server listeners.
        // 1. without_client_cert_verifier() configures no client auth
        // 2. with_alpns() sets the ALPN protocols
        // 3. listen(backlog) creates the listeners (returns Result<Arc<QuicListeners>>)
        let listeners = QuicListeners::builder()
            .without_client_cert_verifier()
            .with_alpns(["h3"])
            .listen(8) // backlog size
            .expect("Failed to create QuicListeners");

        // Add a server with certificate (virtual host style).
        // BindUri accepts &str, ToCertificate/ToPrivateKey accept &Path.
        listeners
            .add_server(
                "localhost",
                cert_path.as_path(),
                key_path.as_path(),
                [in_addr], // SocketAddr implements Into<BindUri>
                None::<Vec<u8>>,
            )
            .await
            .expect("Failed to add server for localhost");

        // Get the actual bound address from the server's interface.
        let listen_addr = {
            let server = listeners.get_server("localhost").expect("Server not found");
            let bind_interfaces = server.bind_interfaces();
            let (_, bind_iface) = bind_interfaces.iter().next().expect("No bound interface");
            bind_iface
                .borrow()
                .bound_addr()
                .expect("Failed to get bound address")
        };

        // listen() already returns Arc<QuicListeners>.
        let acceptor = H3DquicAcceptor::new(listeners);
        let acceptor_cp = acceptor.clone();

        let h_sv = super::run_test_server(acceptor, token);

        let h = tokio::spawn(async move {
            h_sv.await
                .expect("cannot join")
                .expect("tonic server failed");
            // Shutdown the dquic acceptor to release global resources.
            acceptor_cp.shutdown().await;
            // dquic allows only one `QuicListeners` to run at a time globally
            // (its connectionless packet sink is registered on the global
            // `QuicRouter`). Draining it releases the sink so the next test can
            // start a fresh server.
            dquic::qinterface::component::route::QuicRouter::global().drain_connectless();
            tracing::debug!("dquic test server ended");
        });

        (h, listen_addr)
    }
}
