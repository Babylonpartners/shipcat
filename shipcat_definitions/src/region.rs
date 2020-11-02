use crate::structs::kong::Kong;
use std::{collections::BTreeMap, env};

use regex::Regex;

use semver::Version;

use url::Url;
use uuid::Uuid;

#[allow(unused_imports)]
use super::{BaseManifest, ConfigState, Result, Vault};

use super::structs::Authorization;

/// Versioning Scheme used in region
///
/// This is valdiated strictly using `shipcat validate` when versions are found in manifests.
/// Otherwise, it's validated on upgrade time (via `shipcat apply`) when it's passed.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum VersionScheme {
    /// Version must be valid semver (no leading v)
    ///
    /// This is the assumed default for regions that lock versions in manifests.
    Semver,
    /// Version must be valid semver or a 40 character hex (git sha)
    ///
    /// This can be used for rolling environments that does not lock versions in manifests.
    GitShaOrSemver,
}

impl Default for VersionScheme {
    fn default() -> VersionScheme {
        VersionScheme::Semver
    }
}

/// Version validator
impl VersionScheme {
    pub fn verify(&self, ver: &str) -> Result<()> {
        let gitre = Regex::new(r"^[0-9a-f\-]{40}$").unwrap();
        match *self {
            VersionScheme::GitShaOrSemver => {
                if !gitre.is_match(&ver) && Version::parse(&ver).is_err() {
                    bail!("Illegal tag {} (floating tags cannot be rolled back please use 40 char git sha or semver)", ver);
                }
            }
            VersionScheme::Semver => {
                if Version::parse(&ver).is_err() {
                    bail!(
                        "Version {} is not a semver version in a region using semver versions",
                        ver
                    );
                }
            }
        };
        Ok(())
    }
}

/// Vault configuration for a region
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(test, derive(Default))]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct VaultConfig {
    /// Vault url up to and including port
    pub url: String,
    /// Root folder under secret/
    ///
    /// Typically, the name of the region to disambiguate.
    pub folder: String,
}

impl VaultConfig {
    pub fn verify(&self, region: &str) -> Result<()> {
        if self.url == "" {
            bail!("Need to set vault url for {}", region);
        }
        if self.folder == "" {
            bail!("Need to set the vault folder for {}", region);
        }
        if self.folder.contains('/') {
            bail!(
                "vault config folder '{}' (under {}) cannot contain slashes",
                self.folder,
                self.url
            );
        }
        Ok(())
    }

    /// Make vault a vault policy for a team based on team ownership
    ///
    /// Returns plaintext hcl
    #[cfg(feature = "filesystem")]
    pub async fn make_policy(&self, mfs: Vec<BaseManifest>, team: &str, env: Environment) -> Result<String> {
        let mut owned_manifests = vec![];
        for mf in mfs {
            if mf.metadata.team == team {
                owned_manifests.push(mf.name);
            }
        }
        let output = self.template(owned_manifests, env).await?;
        Ok(output)
    }
}

//#[derive(Serialize, Deserialize, Clone, Default)]
//#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
// pub struct HostPort {
//    /// Hostname || IP || FQDN
//    pub host: String,
//    /// Port
//    pub port: u32,
//}

/// Kafka configuration for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KafkaConfig {
    /// Broker urls in "hostname:port" format.
    ///
    /// These are injected in to the manifest.kafka struct if it's set.
    pub brokers: Vec<String>,

    /// Proxy urls in "hostname:port" format.
    ///
    /// These are injected in to the manifest.kafka struct if it's set.
    #[serde(default)]
    pub proxies: Vec<String>,

    /// Zookeeper urls in "hostname:port" format.
    ///
    /// These are injected in to the manifest.kafka struct if it's set.
    #[serde(default)]
    pub zk: Vec<String>,

    /// A mapping of kafka properties to environment variables (optional)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub propertyEnvMapping: BTreeMap<String, String>,
}

/// Webhook types that shipcat might trigger after actions
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "name", deny_unknown_fields, rename_all = "snake_case")]
pub enum Webhook {
    /// Audit webhook details
    Audit(AuditWebhook),
}

/// Where / how to send audited events
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct AuditWebhook {
    /// Endpoint
    pub url: Url,
    /// Credential
    pub token: String,
}

/// Configure how CRs will be deployed on a region
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct CRSettings {
    #[serde(rename = "config")]
    pub shipcatConfig: ConfigState,
}

// ----------------------------------------------------------------------------------

/// Kong configuration for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)] // TODO: better Default impl
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KongConfig {
    /// Base URL to use (e.g. uk.dev.babylontech.co.uk)
    pub base_url: String,
    /// Configuration API URL (e.g. https://kong-admin-ops.dev.babylontech.co.uk)
    pub config_url: String,
    /// Kong token expiration time (in seconds)
    pub kong_token_expiration: u32,
    /// TCP logging options
    pub tcp_log: KongTcpLogConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub jwt_consumers: BTreeMap<String, KongJwtConsumer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub internal_ips_whitelist: Vec<String>,
    #[serde(default, skip_serializing)]
    pub extra_apis: BTreeMap<String, Kong>,
}

/// StatusCake configuration for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct StatuscakeConfig {
    /// Contact Group that will be used if tests go down
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_group: Option<String>,
    /// Extra tags to add to all tests in this region
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_tags: Option<String>,
}

/// Logz.io configuration for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)] // TODO: better Default impl
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct LogzIoConfig {
    /// Base URL to use (e.g. https://app-eu.logz.io/#/dashboard/kibana/dashboard)
    pub url: String,
    /// Account ID (e.g. 46609)
    pub account_id: String,
}

/// Grafana details for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)] // TODO: better Default impl
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct GrafanaConfig {
    /// Base URL to use (e.g. https://dev-grafana.ops.babylontech.co.uk)
    pub url: String,
    /// Services Dashboard ID (e.g. oHzT4g0iz)
    pub services_dashboard_id: String,
}

/// Sentry details for a region
#[derive(Serialize, Deserialize, Clone, Debug, Default)] // TODO: better Default impl
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct SentryConfig {
    /// Base URL to use (e.g. https://dev-uk-sentry.ops.babylontech.co.uk)
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KongAnonymousConsumers {
    pub anonymous: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KongOauthConsumer {
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub username: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KongJwtConsumer {
    pub kid: String,
    pub public_key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct KongTcpLogConfig {
    pub enabled: bool,
    pub host: String,
    pub port: String,
}

impl KongConfig {
    pub fn verify(&self) -> Result<()> {
        Ok(())
    }
}

/// Defaults for services in this region
// TODO: This should be ManifestDefaults from shipcat_filebacked
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct DefaultConfig {
    pub kong: DefaultKongConfig,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct DefaultKongConfig {
    pub authorization: Option<Authorization>,
    // HACK: Authorization doesn't have an enabled property, so this allows authorization to be enabled/disabled on a per region basis until we can use AuthorizationSource.
    pub authorizationEnabled: bool,
}

impl Webhook {
    async fn secrets(&mut self, vault: &Vault, region: &str) -> Result<()> {
        match self {
            Webhook::Audit(h) => {
                if h.token == "IN_VAULT" {
                    let vkey = format!("{}/shipcat/WEBHOOK_AUDIT_TOKEN", region);
                    h.token = vault.read(&vkey).await?;
                }
            }
        }
        Ok(())
    }

    async fn verify_secrets_exist(&self, vault: &Vault, region: &str) -> Result<()> {
        match self {
            Webhook::Audit(_h) => {
                let vkey = format!("{}/shipcat/WEBHOOK_AUDIT_TOKEN", region);
                vault.read(&vkey).await?;
            }
        }
        // TODO: when more secrets, build up a list and do a LIST on shipcat folder
        Ok(())
    }

    pub fn get_configuration(&self) -> Result<BTreeMap<String, String>> {
        let mut whc = BTreeMap::default();
        match self {
            Webhook::Audit(_h) => {
                whc.insert(
                    "SHIPCAT_AUDIT_CONTEXT_ID".into(),
                    env::var("SHIPCAT_AUDIT_CONTEXT_ID").unwrap_or_else(|_| Uuid::new_v4().to_string()),
                );

                // if we are on jenkins
                if let (Ok(url), Ok(revision), Ok(_)) = (
                    env::var("BUILD_URL"),
                    env::var("GIT_COMMIT"),
                    env::var("BUILD_NUMBER"),
                ) {
                    whc.insert("SHIPCAT_AUDIT_REVISION".into(), revision);
                    whc.insert("SHIPCAT_AUDIT_CONTEXT_LINK".into(), url);
                }

                // shipcat evars
                if let Ok(url) = env::var("SHIPCAT_AUDIT_CONTEXT_LINK") {
                    whc.insert("SHIPCAT_AUDIT_CONTEXT_LINK".into(), url);
                }
                if let Ok(revision) = env::var("SHIPCAT_AUDIT_REVISION") {
                    whc.insert("SHIPCAT_AUDIT_REVISION".into(), revision);
                }

                // strict requirements
                if !whc.contains_key("SHIPCAT_AUDIT_REVISION") {
                    return Err("SHIPCAT_AUDIT_REVISION not specified".into());
                }

                debug!("Audit webhook config {:?}", whc);
            }
        }

        // TODO: when slack webhook is cfged, require this:
        // slack::have_credentials()?;

        Ok(whc)
    }
}

#[cfg(test)]
mod test_webhooks {
    use super::{AuditWebhook, Webhook};
    use regex::Regex;
    use std::env;
    use url::Url;

    #[test]
    fn region_webhook_audit_config_jenkins_defaults() {
        let wha = Webhook::Audit(AuditWebhook {
            url: Url::parse("http://testnoop").unwrap(),
            token: "noop".into(),
        });
        let reuuid = Regex::new(r"^[0-9a-f-]{36}$").unwrap();

        // enforce jenkins environment
        env::set_var("GIT_COMMIT", "gc1");
        env::set_var("BUILD_URL", "burl");
        env::set_var("BUILD_NUMBER", "1234");

        let cfg = wha.get_configuration().unwrap();

        assert!(reuuid.is_match(&cfg["SHIPCAT_AUDIT_CONTEXT_ID"]));
        assert_eq!(cfg["SHIPCAT_AUDIT_REVISION"], "gc1");
        assert_eq!(cfg["SHIPCAT_AUDIT_CONTEXT_LINK"], "burl");

        // And in serial, test that shipcat-specific evars trumps it
        env::set_var("SHIPCAT_AUDIT_CONTEXT_ID", "cid1");
        env::set_var("SHIPCAT_AUDIT_CONTEXT_LINK", "burl2");
        env::set_var("SHIPCAT_AUDIT_REVISION", "gc2");

        let cfg = wha.get_configuration().unwrap();

        assert_eq!(cfg["SHIPCAT_AUDIT_CONTEXT_ID"], "cid1");
        assert_eq!(cfg["SHIPCAT_AUDIT_CONTEXT_LINK"], "burl2");
        assert_eq!(cfg["SHIPCAT_AUDIT_REVISION"], "gc2");

        // without revision set up, it should err
        env::remove_var("GIT_COMMIT");
        env::remove_var("SHIPCAT_AUDIT_REVISION");

        let cfg = wha.get_configuration();

        assert!(cfg.is_err());
    }
}

// ----------------------------------------------------------------------------------

/// Environments are well defined strings
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Production environment
    ///
    /// This environment has limited vault access.
    Prod,

    // Normal environment names
    Preprod,
    Staging,
    Dev,
    Test,

    // Misc environments
    Example,
}

#[cfg(test)]
impl Default for Environment {
    fn default() -> Self {
        Environment::Test
    }
}

impl ToString for Environment {
    fn to_string(&self) -> String {
        // NB: this corresponds to serde serialization atm - used in a few places
        format!("{:?}", self).to_lowercase()
    }
}

// ----------------------------------------------------------------------------------

/// Environments are well defined strings
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ReconciliationMode {
    /// Shipcat owned, CRD based decision
    ///
    /// Requires kubernetes 1.13 and above (default).
    /// If CRD was configured, kube apply chart with owner references
    CrdOwned,
}

impl Default for ReconciliationMode {
    fn default() -> Self {
        ReconciliationMode::CrdOwned
    }
}

// ----------------------------------------------------------------------------------

/// A region is an abstract kube context
///
/// Either it's a pure kubernetes context with a namespace and a cluster,
/// or it's an abstract concept with many associated real kubernetes contexts.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(test, derive(Default))]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Region {
    /// Name of region
    pub name: String,
    /// Kubernetes namespace
    pub namespace: String,
    /// Environment (e.g. `dev` or `staging`)
    pub environment: Environment,
    /// Reconciliation mode
    ///
    /// This affects how `cluster crd reconcile` behaves in the region.
    #[serde(default)]
    pub reconciliationMode: ReconciliationMode,

    /// Primary cluster serving this region
    ///
    /// Shipcat does not use this for to decide where a region gets deployed,
    /// but it is used to indicate where the canonical location of a cluster is.
    ///
    /// During blue/green cluster failovers the value of this string may not be accurate.
    ///
    /// Jobs that decide where to deploy a region to should use `get clusterinfo`
    /// with explicit cluster names and regions.
    pub cluster: String,
    /// Versioning scheme
    pub versioningScheme: VersionScheme,

    /// Important base urls that can be templated in evars
    #[serde(default)]
    pub base_urls: BTreeMap<String, String>,

    /// Kong configuration for the region
    #[serde(default)]
    pub kong: Option<KongConfig>,
    /// Statuscake configuration for the region
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuscake: Option<StatuscakeConfig>,
    /// List of Whitelisted IPs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_whitelist: Vec<String>,
    /// Kafka configuration for the region
    #[serde(default)]
    pub kafka: KafkaConfig,
    /// Vault configuration for the region
    pub vault: VaultConfig,
    /// Logz.io configuration for the region
    pub logzio: Option<LogzIoConfig>,
    /// Grafana details for the region
    pub grafana: Option<GrafanaConfig>,
    /// Sentry URL for the region
    pub sentry: Option<SentryConfig>,
    /// List of locations the region serves
    #[serde(default)]
    pub locations: Vec<String>,
    /// All webhooks
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub webhooks: Vec<Webhook>,
    /// CRD tuning
    pub customResources: Option<CRSettings>,

    /// Default values for services (used by shipcat_filebacked only)
    // TODO: Remove `defaultsV2` alias
    #[serde(skip_serializing, default, alias = "defaultsV2")]
    #[cfg(feature = "filesystem")]
    pub defaults: serde_yaml::Value,

    /// The regular expression used to verify destination rules' regions
    #[serde(default, skip_serializing_if = "Option::is_none", with = "serde_regex")]
    pub destinationRuleHostRegex: Option<Regex>,
}

impl Region {
    // Internal secret populator for Config::new
    pub async fn secrets(&mut self) -> Result<()> {
        let v = Vault::regional(&self.vault)?;
        for wh in self.webhooks.iter_mut() {
            wh.secrets(&v, &self.name).await?;
        }
        Ok(())
    }

    // Entry point for region verifier
    pub async fn verify_secrets_exist(&self) -> Result<()> {
        let v = Vault::regional(&self.vault)?;
        for wh in &self.webhooks {
            wh.verify_secrets_exist(&v, &self.name).await?;
        }
        Ok(())
    }

    // Get the Vault URL for a given service in this region
    pub fn vault_url(&self, app: &str) -> String {
        let vault_url = self.vault.url.clone();
        let path = "/ui/vault/secrets/secret/list/";
        format!(
            "{vault_url}/{path}/{env}/{app}/",
            vault_url = vault_url.trim_matches('/'),
            path = path.trim_matches('/'),
            env = &self.name,
            app = &app
        )
    }

    pub fn grafana_url(&self, app: &str) -> Option<String> {
        self.grafana.clone().map(|gf| {
            format!("{grafana_url}/d/{dashboard_id}/kubernetes-services?var-cluster={cluster}&var-namespace={namespace}&var-deployment={app}",
              grafana_url = gf.url.trim_matches('/'),
              dashboard_id = gf.services_dashboard_id,
              app = app,
              cluster = &self.cluster,
              namespace = &self.namespace)
        })
    }

    // Get the Sentry URL for a given service slug in a cluster in this region
    pub fn sentry_url(&self, slug: &str) -> Option<String> {
        self.sentry.clone().map(|s| {
            format!(
                "{sentry_base_url}/sentry/{slug}",
                sentry_base_url = s.url,
                slug = slug
            )
        })
    }

    pub fn logzio_url(&self, app: &str) -> Option<String> {
        self.logzio.clone().map(|lio| {
            format!(
                "{logzio_url}/{app}-{env}?&switchToAccountId={account_id}",
                logzio_url = lio.url.trim_matches('/'),
                app = app,
                env = self.name,
                account_id = lio.account_id
            )
        })
    }

    pub fn raftcat_url(&self) -> Option<String> {
        let devops = String::from("dev-ops");
        let region_name = env::var("REGION_NAME").ok()?;
        if region_name == devops {
            Some(String::from("https://raftcat.ops.babylontech.co.uk/raftcat/"))
        } else {
            self.base_urls
                .get("external_services")
                .map(|base| format!("{base}/raftcat/", base = base))
        }
    }
}
