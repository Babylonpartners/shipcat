use std::{collections::BTreeMap, ops::Not};

use super::Authorization;
use crate::deserializers::comma_separated_string;

/// Kong setup for a service
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Kong {
    /// Auto-populated name of service
    ///
    /// Set internally, and used for defaults (discards value in manifest)
    /// But overrideable for `extra_apis` in `Region::kong`.
    pub name: String,

    /// The base target URL that points to your API server.
    ///
    /// This URL will be used for proxying requests. For example: https://example.com.
    ///
    /// Normal kubernetes value is: raftcat.svc.cluster.local
    /// If left blank, this value will be generated with the service name instead of raftcat.
    pub upstream_url: String,

    /// Whether or not to apply the ip whitelisting (?)
    #[serde(skip_serializing_if = "Not::not")]
    pub internal: bool,

    /// Marker for gate to let external traffic in or not
    #[serde(skip_serializing_if = "Not::not")]
    pub publiclyAccessible: bool,

    /// Authorization for API
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<Authorization>,

    /// Simple path based routing
    ///
    /// E.g. /raftcat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uris: Option<String>,

    /// A comma-separated list of domain names that point to your API.
    ///
    /// For example: example.com. At least one of hosts, uris, or methods should be specified
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "comma_separated_string"
    )]
    pub hosts: Vec<String>,

    pub auth: Option<Authentication>,

    /// When matching an API via one of the uris prefixes, strip that matching prefix from the upstream URI to be requested.
    ///
    /// false => application has to listen on the `uris` parameter (e.g. /raftcat)
    /// true => application has to listen on `/`, but use prefix agnostic urls everywhere.
    pub strip_uri: bool,

    /// Preserves host headers to backend service
    ///
    /// When matching an API via one of the hosts domain names, make sure the request
    /// Host header is forwarded to the upstream service. Kong's default is false,
    /// meaning the upstream Host header will be extracted from the configured upstream_url.
    ///
    /// Shipcat assumes a default of true, as the normal use case is to have this enabled.
    #[serde(default = "preserve_host_default")]
    pub preserve_host: bool,

    /// Configuration parameters for Cross Origin Resource Sharing plugin
    ///
    /// When set, the plugin is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cors: Option<Cors>,

    /// When internal is set to true, also add allow these ips through
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_internal_ips: Vec<String>,

    /// Babyln plugin (Vincent's) for propagating a core-ruby auth header.
    ///
    /// Compatibility layer for old-style core-ruby authorization headers.
    /// Deprecated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub babylon_auth_header: Option<BabylonAuthHeader>,

    /// The timeout in milliseconds for establishing a connection to your upstream service. Defaults to 6000
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_connect_timeout: Option<u32>,

    /// The timeout in milliseconds between two successive write operations for transmitting a request to your upstream service Defaults to 60000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_send_timeout: Option<u32>,

    /// The timeout in milliseconds between two successive read operations for transmitting a request to your upstream service Defaults to 60000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_read_timeout: Option<u32>,

    /// Extra headers to append to the response from kong after reverse proxying
    ///
    /// I.e. the application will receive these extra headers.
    ///
    /// ```yaml
    /// add_headers:
    ///   Cache-Control: no-cache, no-store
    ///   Strict-Transport-Security: max-age=31536000; includeSubDomains; preload;
    ///   X-Content-Type-Options: nosniff
    ///   X-Frame-Options: SAMEORIGIN
    ///   X-XSS-Protection: 1; mode=block
    /// ```
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub add_headers: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_service: Option<String>,

    pub babylon_request_id: bool,
    pub w3c_trace_context: bool,

    pub ip_rate_limits: Option<KongRateLimit>,
    pub user_rate_limits: Option<KongRateLimit>,
}

fn preserve_host_default() -> bool {
    true
}

/// Cors plugin data
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Cors {
    pub credentials: bool,
    pub enabled: bool,
    pub exposed_headers: String,
    pub headers: String,
    pub max_age: String,
    pub methods: String,
    pub origin: String,
    pub preflight_continue: bool,
}

/// Babylon Auth Header plugin data
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct BabylonAuthHeader {
    pub auth_service: String,
    pub cache_timeout_sec: u32,
    pub enabled: bool,
    pub http_timeout_msec: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct KongRateLimit {
    pub per_second: Option<u32>,
    pub per_minute: Option<u32>,
    pub per_hour: Option<u32>,
    pub per_day: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Authentication {
    None,
    Jwt,
}
