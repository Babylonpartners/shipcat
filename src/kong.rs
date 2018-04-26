use serde_json;
use std::io::{self, Write};
use std::collections::BTreeMap;

use super::{Manifest, Result, Config};
use super::structs::Kong;
use super::config::KongConfig;

/// KongOutput matches the format expected by the Kong Configurator script
#[derive(Serialize)]
struct KongOutput {
    pub apis: BTreeMap<String, Kong>,
    pub kong: KongConfig,
}

/// Generate Kong config
///
/// Generate a JSON file used to configure Kong for a given region
pub fn kong_generate(conf: &Config, region: String) -> Result<()> {
    let mut apis = BTreeMap::new();

    // Generate list of APIs to feed to Kong
    for svc in Manifest::available()? {
        debug!("Scanning service {:?}", svc);
        let mf = Manifest::completed(&region, conf, &svc, None)?;
        if !mf.disabled && mf.regions.contains(&region) {
            debug!("Found service {} in region {}", mf.name, region);
            if let Some(k) = mf.kong {
                apis.insert(svc, k);
            }
        }
    }

    // Add general Kong region config
    let reg = conf.regions[&region].clone();
    let kong = reg.kong.clone().unwrap();
    for (name, api) in kong.extra_apis.clone() {
        apis.insert(name, api);
    }
    let output = KongOutput {
        apis: apis,
        kong: reg.kong.unwrap(),
    };

    let _ = io::stdout().write(serde_json::to_string(&output)?.as_bytes());

    Ok(())
}