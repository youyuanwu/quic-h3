//! Mixed backend interop tests.
//!
//! Each server is exercised with both the `quinn` client and the `dquic`
//! client to verify cross-backend interoperability.

use std::{net::SocketAddr, time::Duration};

use serial_test::serial;
use tokio_util::sync::CancellationToken;
use tonic::transport::Uri;

#[tokio::test]
#[serial(dquic)]
async fn h3_quinn_server_test() {
    crate::try_setup_tracing();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = crate::run_test_quinn_hello_server(addr, token.clone());
    h3_test(h_svr, listen_addr, token).await;
}

#[tokio::test]
#[serial(dquic)]
async fn h3_dquic_server_test() {
    crate::try_setup_tracing();
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = crate::dquic_util::run_test_dquic_server(addr, token.clone()).await;
    h3_test(h_svr, listen_addr, token).await;
}

// Sends requests to an already-started server using both the quinn and dquic
// clients to verify cross-backend interoperability.
async fn h3_test(
    h_svr: tokio::task::JoinHandle<()>,
    listen_addr: SocketAddr,
    token: CancellationToken,
) {
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

    // test dquic client
    {
        let quic_client = crate::dquic_util::make_test_dquic_client();
        let server_addr = format!("localhost:{}", listen_addr.port());
        let connection = quic_client.connect(&server_addr).await.unwrap();
        let cc = dquic_h3::H3DquicConnector::new(uri.clone(), "localhost".to_string(), connection);
        let channel = tonic_h3::H3Channel::new(cc, uri.clone(), None);
        let mut client = crate::greeter_client::GreeterClient::new(channel);
        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic-Dquic".into(),
            });
            let response = client.say_hello(request).await.unwrap();
            tracing::debug!("RESPONSE={:?}", response);
        }
    }

    token.cancel();
    h_svr.await.unwrap();
}
