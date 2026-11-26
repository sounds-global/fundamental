//! Exactly one TLS provider is compiled in, so rustls picks it without being told.

#[test]
fn a_client_configuration_is_built_without_naming_a_provider() {
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();

    assert!(
        config
            .crypto_provider()
            .kx_groups
            .iter()
            .any(|group| group.name() == rustls::NamedGroup::X25519MLKEM768),
        "the provider compiled in is not aws-lc-rs: it has no X25519MLKEM768"
    );
}
