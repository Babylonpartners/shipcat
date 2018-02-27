
/// HealthCheck
///
/// Designed for HTTP services for now
/// Used to generate liveness and readiness probes in kubernetes
///
/// If we need complete control over these, consider writing a probes struct
/// and making it only allowed if this is not present.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct HealthCheck {
    /// Where the health check is located
    #[serde(default = "health_check_url_default")]
    pub uri: String,
    /// How long to wait after boot in seconds
    #[serde(default = "health_check_wait_time_default")]
    pub wait: u32,
}
fn health_check_url_default() -> String { "/health".into() }
fn health_check_wait_time_default() -> u32 { 30 }
