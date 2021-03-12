use super::Result;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct HttpGet {
    /// Uri path to GET (i.e. / or /health)
    pub path: String,
    /// Port name (i.e. http or http-health)
    #[serde(default = "http_get_default_port")]
    pub port: String,
    /// Headers to set
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub httpHeaders: Vec<HttpHeader>,
}
fn http_get_default_port() -> String {
    "http".into()
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Exec {
    /// Command to execute in the container
    pub command: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct TcpSocket {
    pub port: String,
}

/// Liveness or readiness Probe
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Probe {
    /// Http Get probe
    #[serde(default, skip_serializing_if = "Option::is_none")]
    httpGet: Option<HttpGet>,

    /// Shell exec probe
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exec: Option<Exec>,

    /// Tcp Socket probe
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tcpSocket: Option<TcpSocket>,

    /// How long to wait before kube performs first probe
    #[serde(default = "initial_delay_seconds_default")]
    pub initialDelaySeconds: u32,

    /// How long between each probe
    #[serde(default = "period_seconds_default")]
    pub periodSeconds: u32,

    /// Min consecutive successes before considering a failed probe successful
    #[serde(default = "success_threshold_default")]
    pub successThreshold: u32,

    /// Min consecutive failures before considering a probe failed
    #[serde(default = "failure_threshold_default")]
    pub failureThreshold: u32,

    /// Number of seconds after which the probe times out
    #[serde(default = "timeout_seconds_default")]
    pub timeoutSeconds: u32,
}

// 5 is kube standard delay default, we set it a little higher
fn initial_delay_seconds_default() -> u32 {
    30
}
// how frequently to poll
fn period_seconds_default() -> u32 {
    5
}
// Default values from Kubernetes
fn success_threshold_default() -> u32 {
    1
}
fn failure_threshold_default() -> u32 {
    3
}
fn timeout_seconds_default() -> u32 {
    1
}

impl Probe {
    pub fn verify(&self) -> Result<()> {
        if self.httpGet.is_some() && (self.exec.is_some() || self.tcpSocket.is_some()) {
            bail!("Probe needs to have at most one of 'httpGet' or 'exec'");
        }
        if self.httpGet.is_none() && self.exec.is_none() && self.tcpSocket.is_none() {
            bail!("Probe needs to define one of 'httpGet', 'exec', 'tcpSocket");
        }
        Ok(())
    }
}
