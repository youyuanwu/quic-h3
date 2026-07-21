//! Mixed backend interop tests.
//!
//! Each server is exercised with both the `quinn` client and the `gm-quic`
//! client to verify cross-backend interoperability.

use std::{net::SocketAddr, time::Duration};

use serial_test::serial;
use tokio_util::sync::CancellationToken;
use tonic::transport::Uri;

#[tokio::test]
#[serial(gm_quic)]
async fn h3_quinn_server_test() {
    h3_test(crate::run_test_quinn_hello_server).await;
}

#[tokio::test]
#[serial(gm_quic)]
async fn h3_gm_quic_server_test() {
    h3_test(crate::gm_quic_util::run_test_gm_quic_server).await;
}

// Takes in the fn to start the server and then sends requests to the server
// using both the quinn and gm-quic clients.
#[allow(clippy::type_complexity)]
async fn h3_test(
    run_server: fn(SocketAddr, CancellationToken) -> (tokio::task::JoinHandle<()>, SocketAddr),
) {
    crate::try_setup_tracing();

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = run_server(addr, token.clone());
    tracing::debug!("listenaddr : {}", listen_addr);

    // send client request
    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::debug!("connecting quic client.");

    let uri: Uri = format!("https://{listen_addr}").parse().unwrap();

    let client_endpoint = crate::make_test_quinn_client_endpoint();
    // quinn client test
    {
        let cc = tonic_h3::quinn::H3QuinnConnector::new(
            uri.clone(),
            "localhost".to_string(),
            client_endpoint.clone(),
        );
        let channel = tonic_h3::H3Channel::new(cc, uri.clone(), None);
        let mut client = crate::greeter_client::GreeterClient::new(channel);

        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic".into(),
            });
            let response = client.say_hello(request).await.unwrap();

            tracing::debug!("RESPONSE={:?}", response);
        }
        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic2".into(),
            });
            let response = client.say_hello(request).await.unwrap();

            tracing::debug!("RESPONSE={:?}", response);
        }
    }
    tracing::debug!("client wait idle");
    client_endpoint.wait_idle().await;

    // test gm-quic client
    {
        let quic_client = crate::gm_quic_util::make_test_gm_quic_client();
        let server_addr = format!("localhost:{}", listen_addr.port());
        let connection = quic_client.connect(&server_addr).await.unwrap();
        let cc = gm_quic_h3::H3GmQuicConnector::new(
            uri.clone(),
            "localhost".to_string(),
            std::sync::Arc::new(connection),
        );
        let channel = tonic_h3::H3Channel::new(cc, uri.clone(), None);
        let mut client = crate::greeter_client::GreeterClient::new(channel);
        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic-GmQuic".into(),
            });
            let response = client.say_hello(request).await.unwrap();
            tracing::debug!("RESPONSE={:?}", response);
        }
    }

    token.cancel();
    h_svr.await.unwrap();
}
