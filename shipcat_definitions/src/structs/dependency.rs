use super::Result;
use std::path::Path;

/// Supported dependency protocols
///
/// Forces lowercase values of this enum to be used
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum DependencyProtocol {
    /// HTTP REST dependency
    Http,
    /// GRPC dependency
    Grpc,
    /// Kafka communication based dependency
    Kafka,
    /// RabbitMQ style dependency
    Amqp,
    /// Amazon SQS style dependency
    Sqs,
}
impl Default for DependencyProtocol {
    fn default() -> DependencyProtocol {
        DependencyProtocol::Http
    }
}

/// Dependency of a service
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Dependency {
    /// Name of service relied upon (used to goto dependent manifest)
    pub name: String,
    /// API version relied upon
    #[serde(default = "default_api_version")]
    pub api: String,
    /// Contract name for dependency
    pub contract: Option<String>,
    /// Protocol/message passing service used to depend on a service
    #[serde(default)]
    pub protocol: DependencyProtocol,
    /// Intent behind dependency - for manifest level descriptiveness
    pub intent: Option<String>,
}

fn default_api_version() -> String {
    "v1".into()
}

impl Dependency {
    pub fn verify(&self) -> Result<()> {
        // self.name must exist in services/
        let dpth = Path::new(".").join("services").join(self.name.clone());
        if !dpth.is_dir() {
            bail!("Service {} does not exist in services/", self.name);
        }
        if self.api != "" {
            let vstr = self.api.chars().skip_while(|ch| *ch == 'v').collect::<String>();
            let ver: usize = vstr.parse()?;
            trace!(
                "Parsed api version of dependency {} as {}",
                self.name.clone(),
                ver
            );
        }
        Ok(())
    }
}
