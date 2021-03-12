use super::{Config, Region};
use std::collections::BTreeMap;

use super::{structs::security::DataHandling, Result};

/// GdprOutput across manifests
#[derive(Serialize)]
struct GdprOutput {
    pub mappings: BTreeMap<String, DataHandling>,
    pub services: Vec<String>,
}

/// Show GDPR related info for a service
///
/// Prints the cascaded structs from a manifests `dataHandling`
pub async fn show(svc: Option<String>, conf: &Config, region: &Region) -> Result<()> {
    let out = if let Some(s) = svc {
        let mf = shipcat_filebacked::load_manifest(&s, conf, region).await?;
        let data = if let Some(dh) = mf.dataHandling {
            dh
        } else {
            DataHandling::default()
        };
        serde_yaml::to_string(&data)?
    } else {
        let mut mappings = BTreeMap::new();
        let mut services = vec![];
        for s in shipcat_filebacked::available(conf, region).await? {
            let mf = shipcat_filebacked::load_manifest(&s.base.name, conf, region).await?;
            if let Some(dh) = mf.dataHandling {
                mappings.insert(s.base.name.clone(), dh);
            }
            services.push(s.base.name);
        }
        let data = GdprOutput { mappings, services };
        serde_yaml::to_string(&data)?
    };
    println!("{}", out);
    Ok(())
}
