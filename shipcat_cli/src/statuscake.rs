use super::{Config, Region, Result};
use shipcat_definitions::{structs::Kong, BaseManifest};

/// One Statuscake object
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct StatuscakeTest {
    #[serde(rename = "name")]
    pub name: String,
    pub website_name: String,
    #[serde(rename = "WebsiteURL")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_group: Option<String>,
    pub test_tags: String,
}

impl StatuscakeTest {
    fn new(region: &Region, mf: &BaseManifest, external_svc: String, kong: Kong) -> Option<Self> {
        let md = &mf.metadata;
        let squad = md.squad.as_ref().expect("squad exists");
        let tribe = md.tribe.as_ref().expect("tribe exists");
        // StatusCake alerts forwarded to pagerduty only includes this name
        // so we have to stuff region, service, and owners into the name :/
        let website_name = format!(
            "{} {} healthcheck squad={},tribe={}",
            region.name, mf.name, squad, tribe
        );

        // Generate the URL to test
        let website_url = if let Some(host) = kong.hosts.first() {
            Some(format!("https://{}/health", host))
        } else if let Some(uris) = kong.uris {
            Some(format!(
                "{}/status/{}/health",
                external_svc,
                uris.trim_start_matches('/')
            ))
        } else {
            // No host, no uri, what's going on?
            None
        };

        // Generate tags, both regional and environment
        // Tags are only helpful for the API part to StatusCake directly
        let mut tags = vec![];
        tags.push(region.name.clone());
        tags.push(region.environment.to_string());
        tags.push(format!("squad={}", squad));
        tags.push(format!("tribe={}", tribe));

        // Process extra region-specific config
        // Set the Contact group if available
        let contact_group = if let Some(ref conf) = region.statuscake {
            if let Some(ref region_tags) = conf.extra_tags {
                tags.push(region_tags.to_string())
            }
            conf.contact_group.clone()
        } else {
            None
        };

        Some(StatuscakeTest {
            name: mf.name.clone(),
            website_name,
            website_url,
            contact_group,
            test_tags: tags.join(","),
        })
    }
}

async fn generate_statuscake_output(conf: &Config, region: &Region) -> Result<Vec<StatuscakeTest>> {
    let mut tests = Vec::new();

    // Ensure the region has a base_url
    if let Some(external_svc) = region.base_urls.get("external_services") {
        debug!("Using base_url.external_services {:?}", external_svc);
        // Generate list of APIs to feed to Statuscake
        for mf in shipcat_filebacked::available(conf, region).await? {
            debug!("Found service {:?}", mf);
            for k in mf.kong_apis.clone() {
                if k.name != mf.base.name {
                    debug!(
                        "{:?} has an additional kong configuration ({:?}), skipping",
                        mf, k.name
                    );
                    continue;
                }
                debug!("{:?} has a main kong configuration, adding", mf);
                if let Some(t) = StatuscakeTest::new(region, &mf.base, external_svc.to_string(), k) {
                    tests.push(t);
                }
            }
        }
    // Extra APIs - let's not monitor them for now (too complex)

    // for (name, api) in region.kong.extra_apis.clone() {
    //    apis.insert(name, api);
    //}
    } else {
        bail!(
            "base_url.external_services is not defined for region {}",
            region.name
        );
    }

    Ok(tests)
}

/// Generate Statuscake config from a filled in global config
pub async fn output(conf: &Config, region: &Region) -> Result<()> {
    let res = generate_statuscake_output(&conf, &region).await?;
    let output = serde_yaml::to_string(&res)?;
    println!("{}", output);

    Ok(())
}
