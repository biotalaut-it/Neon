use neon::agent::model::{LocalModel, ModelConfig};
use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    time::Duration,
};

#[test]
fn refuses_non_loopback_inference_hosts_before_starting_a_process() {
    let config = ModelConfig {
        server_bin: PathBuf::from("/does/not/run"),
        model_path: PathBuf::from("/does/not/exist.gguf"),
        host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        port: 8089,
        context_tokens: 4096,
        threads: 2,
        startup_timeout: Duration::from_millis(1),
        request_timeout: Duration::from_millis(1),
        max_response_bytes: 1024,
    };
    let error = match LocalModel::start(&config) {
        Ok(_) => panic!("non-loopback host must not start"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("loopback"));
}
