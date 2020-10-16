use crate::{
    protos::{
        registration_api::RegisterRequest,
        services::{Plugin, Service},
    },
    Result,
};
use reqwest::Url;
use shipcat_definitions::Manifest;
use std::{collections::HashMap, env};
use tokio::time;

const REGISTER_PATH: &str = "register";
const HEARTBEAT_PATH: &str = "heartbeat";
pub const KOMPASS_AUTH_TOKEN: &str = "KOMPASS_AUTH_TOKEN";

pub fn to_protobuf(m: Manifest, path: &str) -> Service {
    let mut plugins = HashMap::new();
    plugins.insert("raftcat".to_string(), Plugin {
        name: m.name.clone(),
        url: format!("https://{}/raftcat/services/{}", path, m.name),
        icon: "cat".to_string(),
        ..Default::default()
    });
    Service {
        name: m.name,
        version: m.version.unwrap_or_else(|| String::new()),
        namespace: m.namespace,
        cluster: m.region.clone(),
        environment: m.region,
        plugins,
        ..Default::default()
    }
}

pub async fn register(mut kompass_hub_url: Url, raftcat_url: Url) -> Result<()> {
    let mut interval = time::interval(time::Duration::from_secs(30));
    let client = reqwest::Client::new();
    let auth_token = env::var(KOMPASS_AUTH_TOKEN).ok();
    if auth_token.is_none() {
        let err = format_err!("{} not set", KOMPASS_AUTH_TOKEN);
        warn!("{}", err);
        return Err(err);
    }

    let region = env::var("REGION_NAME").expect("Need REGION_NAME evar");
    let ns = env::var("NAMESPACE").expect("Need NAMESPACE evar");
    let id = format!("-raftcat-{}-{}", region, ns);
    let payload = RegisterRequest {
        cluster: region,
        namespace: ns,
        endpoint: raftcat_url.into_string(),
        id,
        ..Default::default()
    };

    kompass_hub_url.set_path(REGISTER_PATH);
    info!("Started register");
    loop {
        interval.tick().await;
        let response = client
            .post(kompass_hub_url.clone())
            .basic_auth("", auth_token.clone())
            .json(&payload)
            .send()
            .await?;

        if response.status().is_success() {
            debug!("Successfully registered with kompass-hub");
            if kompass_hub_url.path() != HEARTBEAT_PATH {
                kompass_hub_url.set_path(HEARTBEAT_PATH);
            }
        } else {
            warn!(
                "Failed to register with kompass-hub: status {}",
                response.status()
            );
            kompass_hub_url.set_path(REGISTER_PATH);
        }
    }
}
