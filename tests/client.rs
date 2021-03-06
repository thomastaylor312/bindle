//! Tests for the client. These tests are not intended to walk through all the API possibilites (as
//! that is taken care of in the API tests), but instead focus on entire user workflows

use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

use bindle::client::Client;
use bindle::testing;

struct TestController {
    pub client: Client,
    server_handle: std::process::Child,
    // Keep a handle to the tempdir so it doesn't drop until the controller drops
    _tempdir: tempfile::TempDir,
}

impl TestController {
    async fn new() -> TestController {
        let build_result = tokio::task::spawn_blocking(|| {
            std::process::Command::new("cargo")
                .args(&["build", "--all-features"])
                .output()
        })
        .await
        .unwrap()
        .expect("unable to run build command");

        assert!(
            build_result.status.success(),
            "Error trying to build server {}",
            String::from_utf8(build_result.stderr).unwrap()
        );

        let tempdir = tempfile::tempdir().expect("unable to create tempdir");

        let address = format!("127.0.0.1:{}", get_random_port());

        let base_url = format!("http://{}/v1/", address);

        let server_handle = std::process::Command::new("cargo")
            .args(&[
                "run",
                "--features",
                "cli",
                "--bin",
                "bindle-server",
                "--",
                "-d",
                tempdir.path().to_string_lossy().to_string().as_str(),
                "-i",
                address.as_str(),
            ])
            .spawn()
            .expect("unable to start bindle server");

        // Give things some time to start up
        tokio::time::delay_for(std::time::Duration::from_secs(2)).await;

        let client = Client::new(&base_url).expect("unable to setup bindle client");
        TestController {
            client,
            server_handle,
            _tempdir: tempdir,
        }
    }
}

impl Drop for TestController {
    fn drop(&mut self) {
        // Not much we can do here if we error, so just ignore
        let _ = self.server_handle.kill();
    }
}

fn get_random_port() -> u16 {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("Unable to bind to check for port")
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn test_successful() {
    // This first creates some invoices/parcels and then tries fetching them to see that they work.
    // Once we confirm that works, we test yank
    let controller = TestController::new().await;

    let scaffold = testing::Scaffold::load("valid_v1").await;

    let inv = controller
        .client
        .create_invoice(scaffold.invoice)
        .await
        .expect("unable to create invoice")
        .invoice;

    controller
        .client
        .get_invoice(&inv.bindle.id)
        .await
        .expect("Should be able to fetch newly created invoice");

    for (k, parcel_data) in scaffold.parcel_files.into_iter() {
        controller
            .client
            .create_parcel(scaffold.labels.get(&k).unwrap().clone(), parcel_data)
            .await
            .expect("Unable to create parcel");
    }

    // Now check that we can get all the parcels
    for label in scaffold.labels.values() {
        controller
            .client
            .get_parcel(&label.sha256)
            .await
            .expect("unable to get parcel");
        // TODO: Should we check content here?
    }

    // TODO: Add test for streaming parcel create after reqwest upgrade

    controller
        .client
        .yank_invoice(&inv.bindle.id)
        .await
        .expect("unable to yank invoice");

    match controller.client.get_invoice(inv.bindle.id).await {
        Ok(_) => panic!("getting a yanked invoice should have errored"),
        Err(e) => {
            if !matches!(e, bindle::client::ClientError::InvoiceNotFound) {
                panic!("Expected an invoice not found error, got: {:?}", e)
            }
        }
    }
}

#[tokio::test]
async fn test_missing() {
    let controller = TestController::new().await;

    // Create a bindle with missing invoices
    let scaffold = testing::Scaffold::load("lotsa_parcels").await;

    let inv = controller
        .client
        .create_invoice(scaffold.invoice)
        .await
        .expect("unable to create invoice")
        .invoice;

    // Check we get the right amount of missing parcels
    let missing = controller
        .client
        .get_missing_parcels(&inv.bindle.id)
        .await
        .expect("Should be able to fetch list of missing parcels");
    assert_eq!(
        missing.len(),
        scaffold.parcel_files.len(),
        "Expected {} missing parcels, found {}",
        scaffold.parcel_files.len(),
        missing.len()
    );

    // Yank the invoice
    controller
        .client
        .yank_invoice(&inv.bindle.id)
        .await
        .expect("unable to yank invoice");

    // Make sure we can't get missing
    match controller.client.get_missing_parcels(&inv.bindle.id).await {
        Ok(_) => panic!("getting a yanked invoice should have errored"),
        Err(e) => {
            if !matches!(e, bindle::client::ClientError::InvoiceNotFound) {
                panic!("Expected an invoice not found error, got: {:?}", e)
            }
        }
    }
}
