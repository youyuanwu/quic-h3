//! dquic integration tests.

use std::time::Duration;

use serial_test::serial;
use tonic::transport::Uri;

use tokio_util::sync::CancellationToken;

/// Test dquic server with quinn client.
#[tokio::test]
#[serial(dquic)]
async fn dquic_server_quinn_client_test() {
    crate::try_setup_tracing();

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = crate::dquic_util::run_test_dquic_server(addr, token.clone()).await;
    tracing::debug!("listenaddr : {}", listen_addr);

    // send client request
    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::debug!("connecting quinn client to dquic server.");

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
                name: "Tonic-Dquic-Quinn".into(),
            });
            let response = client.say_hello(request).await.unwrap();

            tracing::debug!("RESPONSE={:?}", response);
        }
    }
    tracing::debug!("client wait idle");
    client_endpoint.wait_idle().await;

    token.cancel();
    h_svr.await.unwrap();
}

/// Test dquic server with dquic client.
#[tokio::test]
#[serial(dquic)]
async fn dquic_server_dquic_client_test() {
    crate::try_setup_tracing();

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = crate::dquic_util::run_test_dquic_server(addr, token.clone()).await;
    tracing::debug!("listenaddr : {}", listen_addr);

    // send client request
    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::debug!("connecting dquic client to dquic server.");

    let uri: Uri = format!("https://{listen_addr}").parse().unwrap();

    // dquic client test
    {
        let quic_client = crate::dquic_util::make_test_dquic_client();
        let server_addr = format!("localhost:{}", listen_addr.port());
        let connection = quic_client.connect(&server_addr).await.unwrap();
        let cc = dquic_h3::H3DquicConnector::new(uri.clone(), "localhost".to_string(), connection);
        let channel = tonic_h3::H3Channel::new(cc, uri.clone(), None);
        let mut client = crate::greeter_client::GreeterClient::new(channel);

        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic-Dquic-Dquic".into(),
            });
            let response = client.say_hello(request).await.unwrap();

            tracing::debug!("RESPONSE={:?}", response);
        }
    }

    token.cancel();
    h_svr.await.unwrap();
}

/// Test quinn server with dquic client.
#[tokio::test]
#[serial(dquic)]
async fn quinn_server_dquic_client_test() {
    crate::try_setup_tracing();

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let token = CancellationToken::new();
    let (h_svr, listen_addr) = crate::run_test_quinn_hello_server(addr, token.clone());
    tracing::debug!("listenaddr : {}", listen_addr);

    // send client request
    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::debug!("connecting dquic client to quinn server.");

    let uri: Uri = format!("https://{listen_addr}").parse().unwrap();

    // dquic client test
    {
        let quic_client = crate::dquic_util::make_test_dquic_client();
        let server_addr = format!("localhost:{}", listen_addr.port());
        let connection = quic_client.connect(&server_addr).await.unwrap();
        let cc = dquic_h3::H3DquicConnector::new(uri.clone(), "localhost".to_string(), connection);
        let channel = tonic_h3::H3Channel::new(cc, uri.clone(), None);
        let mut client = crate::greeter_client::GreeterClient::new(channel);

        {
            let request = tonic::Request::new(crate::HelloRequest {
                name: "Tonic-Quinn-Dquic".into(),
            });
            let response = client.say_hello(request).await.unwrap();

            tracing::debug!("RESPONSE={:?}", response);
        }
    }

    token.cancel();
    h_svr.await.unwrap();
}
