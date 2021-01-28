// use super::traits::Verify;
use crate::{
    region::KongConfig,
    structs::{Authentication, BabylonAuthHeader, Cors, Kong},
    Region,
};
use serde::ser::{Serialize, SerializeMap, Serializer};
use std::collections::BTreeMap;

/// Kongfig structs
/// https://github.com/mybuilder/kongfig
#[derive(Serialize, Clone, Debug, Default)]
pub struct Api {
    pub name: String,
    pub plugins: Vec<ApiPlugin>,
    pub attributes: ApiAttributes,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ApiAttributes {
    #[serde(serialize_with = "empty_as_brackets")]
    pub hosts: Vec<String>,
    #[serde(serialize_with = "none_as_brackets")]
    pub uris: Option<Vec<String>>,
    #[serde(serialize_with = "none_as_brackets")]
    pub methods: Option<Vec<String>>,
    pub strip_uri: bool,
    pub preserve_host: bool,
    pub upstream_url: String,
    pub retries: u32,
    pub upstream_connect_timeout: u32,
    pub upstream_read_timeout: u32,
    pub upstream_send_timeout: u32,
    pub https_only: bool,
    pub http_if_terminated: bool,
}

/// Plugins and their configs
#[derive(Serialize, Clone, Debug, Default)]
pub struct CorsPluginConfig {
    pub methods: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub max_age: u32,
    pub origins: Vec<String>,
    pub headers: Vec<String>,
    pub credentials: bool,
    pub preflight_continue: bool,
}

impl CorsPluginConfig {
    fn new(cors: Cors) -> Self {
        CorsPluginConfig {
            credentials: cors.credentials,
            exposed_headers: splitter(cors.exposed_headers),
            max_age: cors.max_age.parse().unwrap(),
            methods: splitter(cors.methods),
            origins: splitter(cors.origin),
            headers: splitter(cors.headers),
            preflight_continue: cors.preflight_continue,
        }
    }
}

/// Serialise nil as brackets, a strange kongfig idiom
fn none_as_brackets<S, T>(t: &Option<T>, s: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    match t {
        Some(ref value) => s.serialize_some(value),
        None => s.serialize_map(Some(0))?.end(),
    }
}

/// Serialise empty as brackets.
/// Kong represents an empty list as {}, so Kongfig expects the same to correctly diff the state to work out required changes.
fn empty_as_brackets<S, T>(t: &[T], s: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    if t.is_empty() {
        s.serialize_map(Some(0))?.end()
    } else {
        s.serialize_some(t)
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct HeadersAndJson {
    #[serde(serialize_with = "none_as_brackets")]
    pub headers: Option<Vec<String>>,
    #[serde(serialize_with = "none_as_brackets")]
    pub json: Option<Vec<String>>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ResponseTransformerPluginConfig {
    pub add: HeadersAndJson,
    pub append: HeadersAndJson,
    pub remove: HeadersAndJson,
    pub replace: HeadersAndJson,
}

impl ResponseTransformerPluginConfig {
    fn new(headers: BTreeMap<String, String>) -> Self {
        let mut headers_strs = Vec::new();
        for (k, v) in headers {
            headers_strs.push(format!("{}: {}", k, v));
        }
        ResponseTransformerPluginConfig {
            add: HeadersAndJson {
                headers: Some(headers_strs),
                json: None,
            },
            append: HeadersAndJson::default(),
            remove: HeadersAndJson::default(),
            replace: HeadersAndJson::default(),
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct RequestTransformerPluginConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_method: Option<String>,
    pub remove: HeadersQueryBody,
    pub replace: HeadersQueryBody,
    pub add: HeadersQueryBody,
    pub append: HeadersQueryBody,
    pub rename: HeadersQueryBody,
}

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct HeadersQueryBody {
    #[serde(serialize_with = "none_as_brackets")]
    pub querystring: Option<Vec<String>>,
    #[serde(serialize_with = "none_as_brackets")]
    pub headers: Option<Vec<String>>,
    #[serde(serialize_with = "none_as_brackets")]
    pub body: Option<Vec<String>>,
}

impl RequestTransformerPluginConfig {
    fn new(headers: BTreeMap<String, String>) -> Self {
        let mut headers_strs = Vec::new();
        for (k, v) in headers {
            headers_strs.push(format!("{}: {}", k, v));
        }
        let mut xs = Self::default();
        xs.add.headers = Some(headers_strs.clone());
        xs.replace.headers = Some(headers_strs);
        xs
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct TcpLogPluginConfig {
    pub host: String,
    pub port: u32,
    pub keepalive: u32,
    pub timeout: u32,
}

impl TcpLogPluginConfig {
    fn new(host: &str, port: u32) -> Self {
        TcpLogPluginConfig {
            host: host.into(),
            port: port,
            keepalive: 60000,
            timeout: 10000,
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct JwtPluginConfig {
    pub key_claim_name: String,
    #[serde(serialize_with = "empty_as_brackets")]
    pub claims_to_verify: Vec<String>,

    pub secret_is_base64: bool,
    pub run_on_preflight: bool,

    #[serde(serialize_with = "empty_as_brackets")]
    pub uri_param_names: Vec<String>,
    #[serde(serialize_with = "empty_as_brackets")]
    pub cookie_names: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous: Option<String>,
}
impl JwtPluginConfig {
    fn new(anonymous_consumer: Option<String>) -> Self {
        JwtPluginConfig {
            uri_param_names: vec![],
            cookie_names: vec![],

            claims_to_verify: vec!["exp".into()],
            key_claim_name: "kid".into(),

            anonymous: match anonymous_consumer.clone() {
                Some(_s) => None,
                None => Some("".into()),
            },
            anonymous_username: anonymous_consumer.map(|_| "anonymous".into()),
            secret_is_base64: false,
            run_on_preflight: true,
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct JwtValidatorPluginConfig {
    pub allowed_audiences: Vec<String>,
    pub expected_region: String,
    pub expected_scope: String,
    pub allow_invalid_tokens: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct JsonCookiesCsrfPluginConfig {}

impl Default for JsonCookiesCsrfPluginConfig {
    fn default() -> Self {
        JsonCookiesCsrfPluginConfig {}
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct JsonCookiesToHeadersPluginConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_refresh_token_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_max_age_sec: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_refresh_expired_access_tokens: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_timeout_msec: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renew_before_expiry_sec: Option<u32>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct BabylonAuthHeaderPluginConfig {
    pub auth_service: String,
    pub cache_timeout_sec: u32,
    pub http_timeout_msec: u32,
}

impl BabylonAuthHeaderPluginConfig {
    fn new(authheader: BabylonAuthHeader) -> Self {
        BabylonAuthHeaderPluginConfig {
            auth_service: authheader.auth_service,
            cache_timeout_sec: authheader.cache_timeout_sec,
            http_timeout_msec: authheader.http_timeout_msec,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct CorrelationIdPluginConfig {
    pub echo_downstream: bool,
    pub header_name: String,
    pub generator: String,
}

impl Default for CorrelationIdPluginConfig {
    fn default() -> Self {
        CorrelationIdPluginConfig {
            echo_downstream: true,
            header_name: "babylon-request-id".into(),
            generator: "uuid".into(),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct W3CTraceContextPluginConfig {
    pub set_babylon_request_id: bool,
}

impl Default for W3CTraceContextPluginConfig {
    fn default() -> Self {
        W3CTraceContextPluginConfig {
            set_babylon_request_id: true,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct UserRateLimitPluginConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u32>,
    pub policy: String,
    pub fault_tolerant: bool,
    pub hide_client_headers: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_host: Option<String>,
    pub redis_port: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_password: Option<String>,
    pub redis_timeout: u32,
    pub redis_database: u32,
}

// https://github.com/Kong/kong/blob/4973a6237b108f0b332ca97b187faf669f2497dd/kong/plugins/rate-limiting/schema.lua#L7-L21
#[derive(Serialize, Debug, Clone)]
pub struct RateLimitingPluginConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u32>,
    pub limit_by: String,
    pub policy: String,
    pub fault_tolerant: bool,
    pub hide_client_headers: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_host: Option<String>,
    pub redis_port: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_password: Option<String>,
    pub redis_timeout: u32,
    pub redis_database: u32,
}

#[allow(clippy::large_enum_variant)] // variants all reasonably similar
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "name", rename_all = "kebab-case")]
pub enum ApiPlugin {
    TcpLog(PluginBase<TcpLogPluginConfig>),
    Jwt(PluginBase<JwtPluginConfig>),
    JwtValidator(PluginBase<JwtValidatorPluginConfig>),
    Cors(PluginBase<CorsPluginConfig>),
    CorrelationId(PluginBase<CorrelationIdPluginConfig>),
    // serde's kebab case conversion adds a hyphen after the "3" (even with `W3cTraceContext`)
    #[serde(rename = "w3c-trace-context")]
    W3CTraceContext(PluginBase<W3CTraceContextPluginConfig>),
    BabylonAuthHeader(PluginBase<BabylonAuthHeaderPluginConfig>),
    JsonCookiesToHeaders(PluginBase<JsonCookiesToHeadersPluginConfig>),
    JsonCookiesCsrf(PluginBase<JsonCookiesCsrfPluginConfig>),
    ResponseTransformer(PluginBase<ResponseTransformerPluginConfig>),
    RequestTransformer(PluginBase<RequestTransformerPluginConfig>),
    RateLimiting(PluginBase<RateLimitingPluginConfig>),
    UserRateLimit(PluginBase<UserRateLimitPluginConfig>),
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "ensure", content = "attributes", rename_all = "lowercase")]
pub enum PluginBase<T> {
    Present(PluginAttributes<T>),
    Removed,
}

impl<T: Default> Default for PluginBase<T> {
    fn default() -> Self {
        PluginBase::new(T::default())
    }
}

impl<T> PluginBase<T> {
    fn new(config: T) -> Self {
        PluginBase::Present(PluginAttributes {
            enabled: true,
            config: config,
        })
    }

    fn removed() -> Self {
        PluginBase::Removed
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct PluginAttributes<T> {
    pub enabled: bool,
    pub config: T,
}

impl<T: Default> Default for PluginAttributes<T> {
    fn default() -> Self {
        PluginAttributes {
            enabled: true,
            config: T::default(),
        }
    }
}

fn splitter(value: String) -> Vec<String> {
    value.split(',').map(|h| h.trim()).map(String::from).collect()
}

pub fn kongfig_apis(from: BTreeMap<String, Kong>, config: KongConfig, region: &Region) -> Vec<Api> {
    let mut apis = Vec::new();
    for (k, v) in from.clone() {
        let mut plugins = Vec::new();

        plugins.push(ApiPlugin::CorrelationId(if v.babylon_request_id {
            PluginBase::default()
        } else {
            PluginBase::removed()
        }));

        plugins.push(ApiPlugin::W3CTraceContext(if v.w3c_trace_context {
            PluginBase::new(W3CTraceContextPluginConfig {
                set_babylon_request_id: v.babylon_request_id,
            })
        } else {
            PluginBase::removed()
        }));

        // If globally enabled: TCP Logging
        if config.tcp_log.enabled {
            plugins.push(ApiPlugin::TcpLog(PluginBase::new(TcpLogPluginConfig::new(
                &config.tcp_log.host,
                config.tcp_log.port.parse().unwrap(),
            ))));
        }

        if let Some(a) = v.authorization {
            plugins.push(ApiPlugin::Jwt(PluginBase::new(JwtPluginConfig::new(
                if a.allow_anonymous {
                    Some("anonymous".to_string())
                } else {
                    None
                },
            ))));
            plugins.push(ApiPlugin::JwtValidator(PluginBase::new(
                JwtValidatorPluginConfig {
                    allowed_audiences: a.allowed_audiences,
                    expected_scope: a.required_scopes.get(0).map_or("".to_string(), |s| s.to_string()),
                    allow_invalid_tokens: a.allow_invalid_tokens,
                    expected_region: region.name.clone(),
                },
            )));
            if a.allow_cookies {
                plugins.push(ApiPlugin::JsonCookiesToHeaders(PluginBase::new(
                    JsonCookiesToHeadersPluginConfig {
                        auth_service: a.refresh_auth_service,
                        body_refresh_token_key: a.refresh_body_refresh_token_key,
                        cookie_max_age_sec: a.refresh_max_age_sec,
                        cookie_domain: a.refresh_cookie_domain,
                        enable_refresh_expired_access_tokens: Some(a.enable_cookie_refresh),
                        http_timeout_msec: a.refresh_http_timeout_msec,
                        renew_before_expiry_sec: a.refresh_renew_before_expiry_sec,
                    },
                )));
                plugins.push(ApiPlugin::JsonCookiesCsrf(PluginBase::default()));
            } else {
                plugins.push(ApiPlugin::JsonCookiesToHeaders(PluginBase::removed()));
                plugins.push(ApiPlugin::JsonCookiesCsrf(PluginBase::removed()));
            }
        } else {
            // JWT plugin
            plugins.push(ApiPlugin::Jwt(match v.auth {
                Some(Authentication::Jwt) => PluginBase::new(JwtPluginConfig::new(None)),
                _ => PluginBase::removed(),
            }));
            plugins.push(ApiPlugin::JwtValidator(PluginBase::removed()));
        }

        plugins.push(ApiPlugin::RateLimiting(if let Some(limits) = v.ip_rate_limits {
            PluginBase::new(RateLimitingPluginConfig {
                minute: limits.per_minute,
                hour: limits.per_hour,
                day: limits.per_day,
                limit_by: "ip".to_string(),
                policy: "cluster".to_string(),
                fault_tolerant: true,
                hide_client_headers: true,
                redis_host: None,
                redis_port: 6379,
                redis_password: None,
                redis_timeout: 2000,
                redis_database: 0,
            })
        } else {
            PluginBase::removed()
        }));

        plugins.push(ApiPlugin::UserRateLimit(
            if let Some(limits) = v.user_rate_limits {
                PluginBase::new(UserRateLimitPluginConfig {
                    minute: limits.per_minute,
                    hour: limits.per_hour,
                    day: limits.per_day,
                    policy: "cluster".to_string(),
                    fault_tolerant: true,
                    hide_client_headers: true,
                    redis_host: None,
                    redis_port: 6379,
                    redis_password: None,
                    redis_timeout: 2000,
                    redis_database: 0,
                })
            } else {
                PluginBase::removed()
            },
        ));

        // Babylon Auth Header plugin
        // TODO: Remove plugin if not enabled/None
        if let Some(babylon_auth_header) = v.babylon_auth_header {
            let plugin = PluginBase::Present(PluginAttributes {
                enabled: babylon_auth_header.enabled,
                config: BabylonAuthHeaderPluginConfig::new(babylon_auth_header),
            });
            plugins.push(ApiPlugin::BabylonAuthHeader(plugin));
        }

        // If enabled: CORS
        if let Some(cors) = v.cors {
            plugins.push(ApiPlugin::Cors(PluginBase::Present(PluginAttributes {
                // TODO: Remove plugin if not enabled/None
                enabled: cors.enabled,
                config: CorsPluginConfig::new(cors),
            })));
        }

        // If enabled: ResponseTransformer to add headers
        if !v.add_headers.is_empty() {
            plugins.push(ApiPlugin::ResponseTransformer(PluginBase::new(
                ResponseTransformerPluginConfig::new(v.add_headers),
            )));
        }

        if let Some(upstream_service) = v.upstream_service {
            plugins.push(ApiPlugin::RequestTransformer(PluginBase::new(
                RequestTransformerPluginConfig::new(btreemap! {
                    "Upstream-Service".into() => upstream_service,
                }),
            )))
        } else {
            plugins.push(ApiPlugin::RequestTransformer(PluginBase::removed()))
        }

        // Create the main API object
        apis.push(Api {
            name: k.to_string(),
            plugins: plugins,
            attributes: ApiAttributes {
                hosts: v.hosts,
                uris: v.uris.map(|s| vec![s]),
                preserve_host: v.preserve_host,
                strip_uri: v.strip_uri,
                upstream_connect_timeout: v.upstream_connect_timeout.unwrap_or(30000),
                upstream_read_timeout: v.upstream_read_timeout.unwrap_or(30000),
                upstream_send_timeout: v.upstream_send_timeout.unwrap_or(30000),
                upstream_url: v.upstream_url,
                ..Default::default()
            },
        });
    }
    apis
}

pub fn kongfig_consumers(k: KongConfig) -> Vec<Consumer> {
    let mut consumers: Vec<Consumer> = k
        .jwt_consumers
        .into_iter()
        .map(|(k, v)| Consumer {
            username: k,
            acls: vec![],
            credentials: vec![ConsumerCredentials::Jwt(JwtCredentialsAttributes {
                key: v.kid,
                algorithm: "RS256".into(),
                rsa_public_key: v.public_key,
            })],
        })
        .collect();

    // Add the anonymous customer as well
    consumers.push(Consumer {
        username: "anonymous".into(),
        acls: vec![],
        credentials: vec![],
    });

    consumers
}

#[derive(Serialize, Debug, Clone)]
pub struct Consumer {
    pub username: String,
    pub acls: Vec<String>,
    pub credentials: Vec<ConsumerCredentials>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "name", content = "attributes", rename_all = "kebab-case")]
pub enum ConsumerCredentials {
    Jwt(JwtCredentialsAttributes),
}

#[derive(Serialize, Debug, Clone)]
pub struct JwtCredentialsAttributes {
    pub algorithm: String,
    pub key: String,
    pub rsa_public_key: String,
}

/// Not used yet
#[derive(Serialize, Debug, Clone, Default)]
pub struct Plugin {}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Upstream {}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Certificate {}
