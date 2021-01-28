use merge::Merge;
use std::collections::BTreeMap;

use shipcat_definitions::{
    structs::{Authentication, Authorization, BabylonAuthHeader, Cors, Kong, KongRateLimit},
    KongConfig, Region, Result,
};

use super::{
    authorization::AuthorizationSource,
    util::{Build, Enabled, EnabledMap},
};

#[derive(Deserialize, Default, Merge, Clone)]
#[serde(default)]
pub struct KongApisSource {
    /// Default values to merge into every API
    pub defaults: KongSource,

    #[serde(flatten)]
    pub apis: EnabledMap<String, KongSource>,
}

pub struct KongApisBuildParams {
    pub service: String,
    pub region: Region,
    pub kong: KongConfig,
    // TODO: Remove Manifest.kong
    pub single_api: Enabled<KongSource>,
}

impl Build<Vec<Kong>, KongApisBuildParams> for KongApisSource {
    fn build(self, params: &KongApisBuildParams) -> Result<Vec<Kong>> {
        let defaults = Enabled {
            enabled: None,
            item: self.defaults,
        };

        if let Some(k) = KongApisSource::build_single_api(&defaults, params)? {
            debug!("Using single Kong API for {}", params.service);
            if !self.apis.is_empty() {
                bail!(".kong and .kong_apis properties are mutually exclusive")
            }
            return Ok(vec![k]);
        }

        let mut built = Vec::new();
        for (name, k) in self.apis {
            debug!("Building Kong API {}", &name);
            if name != params.service && !name.starts_with(&format!("{}-", &params.service)) {
                // TODO: Duplicate Kong's name validation
                bail!("Kong API name must be '${SERVICE_NAME}' or '${SERVICE_NAME}-*'")
            }
            let merged = defaults.clone().merge(k);
            let maybe = merged.build(&KongBuildParams {
                name,
                service: params.service.clone(),
                region: params.region.clone(),
                kong: params.kong.clone(),
            })?;
            if let Some(api) = maybe {
                built.push(api);
            }
        }
        Ok(built)
    }
}

impl KongApisSource {
    fn build_single_api(
        defaults: &Enabled<KongSource>,
        params: &KongApisBuildParams,
    ) -> Result<Option<Kong>> {
        let Enabled {
            enabled,
            item: merged,
        } = defaults.clone().merge(params.single_api.clone());
        if let Some(false) = enabled {
            return Ok(None);
        }

        // For backwards compatibility, { uris: null, hosts: [], ... } is equivalent to { enabled: false, ... }
        if merged.hosts.clone().unwrap_or_default().is_empty() && merged.uris.is_none() {
            return Ok(None);
        }

        Ok(Some(merged.build(&KongBuildParams {
            name: params.service.clone(),
            service: params.service.clone(),
            region: params.region.clone(),
            kong: params.kong.clone(),
        })?))
    }
}

#[derive(Deserialize, Default, Merge, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct KongSource {
    pub upstream_url: Option<String>,
    pub uris: Option<String>,
    pub hosts: Option<Vec<String>>,
    pub strip_uri: Option<bool>,
    pub preserve_host: Option<bool>,
    pub cors: Option<Cors>,
    pub additional_internal_ips: Option<Vec<String>>,

    pub internal: Option<bool>,
    #[serde(rename = "camelCase")]
    pub publicly_accessible: Option<bool>,
    pub auth: Option<Authentication>,
    pub babylon_auth_header: Option<BabylonAuthHeader>,
    pub authorization: Enabled<AuthorizationSource>,

    pub upstream_connect_timeout: Option<u32>,
    pub upstream_send_timeout: Option<u32>,
    pub upstream_read_timeout: Option<u32>,
    pub add_headers: BTreeMap<String, String>,

    pub w3c_trace_context: Option<bool>,
    pub babylon_request_id: Option<bool>,

    pub ip_rate_limits: Enabled<KongRateLimitSource>,
    pub user_rate_limits: Enabled<KongRateLimitSource>,
}

struct KongBuildParams {
    pub name: String,
    pub service: String,
    pub region: Region,
    pub kong: KongConfig,
}

impl Build<Kong, KongBuildParams> for KongSource {
    /// Build a Kong from a KongSource, validating and mutating properties.
    fn build(self, params: &KongBuildParams) -> Result<Kong> {
        let KongBuildParams {
            region,
            service,
            name,
            kong,
        } = params;
        debug!("Building Kong API {} for {}", &name, &service);

        let hosts = self.build_hosts(&kong.base_url)?;
        if hosts.is_empty() && self.uris.is_none() {
            bail!("At least one of hosts or uris must be set on a Kong API")
        }

        let upstream_url = self.build_upstream_url(&service, &region.namespace);
        let (auth, authorization) = KongSource::build_auth(self.auth, self.authorization)?;

        let preserve_host = self.preserve_host.unwrap_or(true);

        Ok(Kong {
            name: name.to_string(),
            upstream_url: upstream_url,
            upstream_service: if preserve_host {
                Some(service.to_string())
            } else {
                None
            },
            internal: self.internal.unwrap_or_default(),
            publiclyAccessible: self.publicly_accessible.unwrap_or_default(),
            uris: self.uris,
            hosts,
            authorization,
            strip_uri: self.strip_uri.unwrap_or_default(),
            preserve_host,
            cors: self.cors,
            additional_internal_ips: self.additional_internal_ips.unwrap_or_default(),
            babylon_auth_header: self.babylon_auth_header,
            upstream_connect_timeout: self.upstream_connect_timeout,
            upstream_send_timeout: self.upstream_send_timeout,
            upstream_read_timeout: self.upstream_read_timeout,
            add_headers: self.add_headers,
            // Legacy authorization
            auth,
            // Distributed Tracing
            babylon_request_id: self.babylon_request_id.unwrap_or(true), // enabled by default for backwards compatibility.
            w3c_trace_context: self.w3c_trace_context.unwrap_or_default(),

            ip_rate_limits: self.ip_rate_limits.build(&())?,
            user_rate_limits: self.user_rate_limits.build(&())?,
        })
    }
}

impl KongSource {
    fn build_upstream_url(&self, service: &str, namespace: &str) -> String {
        if let Some(upstream_url) = &self.upstream_url {
            upstream_url.to_string()
        } else {
            format!("http://{}.{}.svc.cluster.local", service, namespace)
        }
    }

    fn build_auth(
        auth: Option<Authentication>,
        authz: Enabled<AuthorizationSource>,
    ) -> Result<(Option<Authentication>, Option<Authorization>)> {
        Ok(match (auth, authz.build(&())?) {
            (Some(_), Some(_)) => bail!("auth and authorization.enabled are mutually exclusive"),
            (Some(Authentication::None), None) => (None, None),
            x => x,
        })
    }

    fn build_hosts(&self, base_url: &str) -> Result<Vec<String>> {
        Ok(self
            .hosts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|h| {
                let fully_qualified = h.contains('.');
                if fully_qualified {
                    h
                } else {
                    format!("{}{}", h, base_url)
                }
            })
            .collect())
    }
}

#[derive(Deserialize, Default, Merge, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct KongRateLimitSource {
    pub per_second: Option<u32>,
    pub per_minute: Option<u32>,
    pub per_hour: Option<u32>,
    pub per_day: Option<u32>,
}

impl Build<KongRateLimit, ()> for KongRateLimitSource {
    fn build(self, _params: &()) -> Result<KongRateLimit> {
        Ok(KongRateLimit {
            per_second: self.per_second,
            per_minute: self.per_minute,
            per_hour: self.per_hour,
            per_day: self.per_day,
        })
    }
}
