#![allow(non_snake_case)]

use merge::Merge;
use std::collections::BTreeMap;

use shipcat_definitions::{
    structs::{
        autoscaling::AutoScaling,
        metadata::{default_format_string, Contact, Context, Language, SlackChannel},
        security::DataHandling,
        tolerations::Tolerations,
        volume::Volume,
        ConfigMap, Dependency, DestinationRule, EventStream, Gate, HealthCheck, HostAlias, Kafka,
        KafkaResources, LifeCycle, Metadata, NotificationMode, PersistentVolume, Probe, PrometheusAlert,
        Rbac, RollingUpdate, SecurityContext, VaultOpts, VolumeMount,
    },
    BaseManifest, Config, Manifest, PrimaryWorkload, Region, Result,
};

use super::{
    container::{
        ContainerBuildParams, CronJobSource, EnvVarsSource, ImageNameSource, ImageTagSource,
        InitContainerSource, PortSource, ResourceRequirementsSource, SidecarSource, WorkerSource,
    },
    kong::{KongApisBuildParams, KongApisSource, KongSource},
    newrelic_source::NewrelicSource,
    sentry_source::SentrySource,
    util::{Build, Enabled, RelaxedString, Require},
    SimpleManifest,
};

/// Helper for optional string/list of string structs
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> Default for OneOrMany<T> {
    fn default() -> OneOrMany<T> {
        OneOrMany::Many(vec![])
    }
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct MetadataSource {
    pub repo: String,
    pub team: String,
    pub context: Option<Context>,
    #[serde(skip_deserializing)]
    pub squad: Option<String>,
    #[serde(skip_deserializing)]
    pub tribe: Option<String>,
    pub language: Option<Language>,
    #[serde(default = "default_format_string")]
    pub gitTagTemplate: String,
    pub contacts: Vec<Contact>,
    pub maintainers: Vec<String>,
    pub support: Option<SlackChannel>,
    pub notifications: Option<SlackChannel>,
    pub runbook: Option<String>,
    pub description: Option<String>,
    pub docs: Option<String>,

    pub ped: Option<String>,
    pub testPlan: Option<String>,
    pub releasePlan: Option<String>,
    pub threatModel: OneOrMany<String>,
    pub dpsia: OneOrMany<String>,

    // TODO: generate swagger docs url from region and service name
    /// Custom metadata, keys defined in the Config
    #[serde(flatten)]
    pub custom: BTreeMap<String, String>,
}

/// Main manifest, deserialized from `manifest.yml`
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ManifestSource {
    pub name: Option<String>,
    pub external: bool,
    pub disabled: bool,
    pub regions: Vec<String>,
    pub metadata: Option<MetadataSource>,

    #[serde(flatten)]
    pub overrides: ManifestOverrides,
}

/// Manifest overrides, deserialized from `dev-uk.yml`/`prod.yml` etc.
#[derive(Deserialize, Default, Merge, Clone)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestOverrides {
    pub workload: Option<PrimaryWorkload>,
    pub publicly_accessible: Option<bool>,
    pub kompass_plugin: Option<bool>,
    pub image: Option<ImageNameSource>,
    pub image_size: Option<u32>,
    pub version: Option<ImageTagSource>,
    pub command: Option<Vec<String>>,
    pub security_context: Option<SecurityContext>,
    pub data_handling: Option<DataHandling>,
    pub resources: Option<ResourceRequirementsSource>,
    pub secret_files: BTreeMap<String, String>,
    pub configs: Option<ConfigMap>,
    pub vault: Option<VaultOpts>,
    pub http_port: Option<u32>,
    pub ports: Option<Vec<PortSource>>,
    pub external_port: Option<u32>,
    pub health: Option<HealthCheck>,
    pub dependencies: Option<Vec<Dependency>>,
    pub destination_rules: Option<Vec<DestinationRule>>,
    pub workers: Option<Vec<WorkerSource>>,
    pub sidecars: Option<Vec<SidecarSource>>,
    pub readiness_probe: Option<Probe>,
    pub liveness_probe: Option<Probe>,
    pub lifecycle: Option<LifeCycle>,
    pub rolling_update: Option<RollingUpdate>,
    pub auto_scaling: Option<AutoScaling>,
    pub tolerations: Option<Vec<Tolerations>>,
    pub host_aliases: Option<Vec<HostAlias>>,
    pub init_containers: Option<Vec<InitContainerSource>>,
    pub volumes: Option<Vec<Volume>>,
    pub volume_mounts: Option<Vec<VolumeMount>>,
    pub persistent_volumes: Option<Vec<PersistentVolume>>,
    pub cron_jobs: Option<Vec<CronJobSource>>,
    pub service_annotations: BTreeMap<String, String>,
    pub pod_annotations: BTreeMap<String, RelaxedString>,
    pub labels: BTreeMap<String, RelaxedString>,
    pub gate: Option<Gate>,
    pub kafka: Option<Kafka>,
    pub source_ranges: Option<Vec<String>>,
    pub rbac: Option<Vec<Rbac>>,
    pub sentry: Option<SentrySource>,
    pub event_streams: Option<Vec<EventStream>>,
    pub kafka_resources: Option<KafkaResources>,
    //  to have this section merge alerts sub-field deeply
    //      we have to avoid using Option
    pub newrelic: NewrelicSource,
    pub upgrade_notifications: Option<NotificationMode>,
    pub prometheus_alerts: Option<Vec<PrometheusAlert>>,

    #[serde(flatten)]
    pub defaults: ManifestDefaults,
}

/// Global/regional manifest defaults, deserialized from `shipcat.conf` etc.
#[derive(Deserialize, Default, Merge, Clone)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestDefaults {
    pub image_prefix: Option<String>,
    pub chart: Option<String>,
    pub replica_count: Option<u32>,
    pub env: EnvVarsSource,
    pub kong_apis: KongApisSource,
    // TODO: Migrate to kong_apis
    pub kong: Enabled<KongSource>,
}

// impl Build<Manifest, (Config, Region)> - but no need to have this as a trait
impl ManifestSource {
    /// Build a Manifest from a ManifestSource, validating and mutating properties.
    pub async fn build(self, (conf, region): &(Config, Region)) -> Result<Manifest> {
        let simple = self.build_simple(conf, region)?;
        let name = simple.base.name;
        let data_handling = self.build_data_handling();
        let kafka = self.build_kafka(&name, region);
        let configs = self.build_configs(&name).await?;

        let overrides = self.overrides;
        let defaults = overrides.defaults;

        let container_build_params = ContainerBuildParams {
            main_envs: defaults.env.clone(),
        };

        let team_notifications = simple
            .base
            .metadata
            .clone()
            .notifications
            .expect("notifications channel is always defined");

        Ok(Manifest {
            name,
            publiclyAccessible: overrides.publicly_accessible.unwrap_or_default(),
            kompass_plugin: overrides.kompass_plugin.unwrap_or_default(),
            // TODO: Skip most validation if true
            external: simple.external,
            // TODO: Replace with simple.enabled
            disabled: self.disabled,
            // TODO: Must be non-empty
            regions: simple.base.regions,
            // TODO: Make metadata non-optional
            metadata: Some(simple.base.metadata),
            chart: defaults.chart,
            // TODO: Make imageSize non-optional
            imageSize: overrides.image_size.or(Some(512)),
            image: simple.image,
            version: simple.version,
            command: overrides.command.unwrap_or_default(),
            securityContext: overrides.security_context,
            dataHandling: data_handling,
            resources: overrides.resources.build(&())?,
            replicaCount: defaults.replica_count,
            env: defaults.env.build(&())?,
            secretFiles: overrides.secret_files,
            configs: configs,
            vault: overrides.vault,
            httpPort: overrides.http_port,
            ports: overrides.ports.unwrap_or_default().build(&())?,
            externalPort: overrides.external_port,
            health: overrides.health,
            dependencies: overrides.dependencies.unwrap_or_default(),
            destinationRules: overrides.destination_rules,
            workers: overrides
                .workers
                .unwrap_or_default()
                .build(&container_build_params)?,
            sidecars: overrides
                .sidecars
                .unwrap_or_default()
                .build(&container_build_params)?,
            readinessProbe: overrides.readiness_probe,
            livenessProbe: overrides.liveness_probe,
            lifecycle: overrides.lifecycle,
            rollingUpdate: overrides.rolling_update,
            autoScaling: overrides.auto_scaling,
            tolerations: overrides.tolerations.unwrap_or_default(),
            hostAliases: overrides.host_aliases.unwrap_or_default(),
            initContainers: overrides
                .init_containers
                .unwrap_or_default()
                .build(&container_build_params)?,
            volumes: overrides.volumes.unwrap_or_default(),
            volumeMounts: overrides.volume_mounts.unwrap_or_default(),
            persistentVolumes: overrides.persistent_volumes.unwrap_or_default(),
            cronJobs: overrides
                .cron_jobs
                .unwrap_or_default()
                .build(&container_build_params)?,
            serviceAnnotations: overrides.service_annotations,
            podAnnotations: overrides.pod_annotations.build(&())?,
            labels: overrides.labels.build(&())?,
            kongApis: simple.kong_apis,
            gate: overrides.gate,
            kafka: kafka,
            sourceRanges: overrides.source_ranges.unwrap_or_default(),
            rbac: overrides.rbac.unwrap_or_default(),
            newrelic: overrides.newrelic.build(&team_notifications)?,
            sentry: overrides
                .sentry
                .map(|sentry| sentry.build(&team_notifications))
                .transpose()?,
            eventStreams: overrides.event_streams.unwrap_or_default(),
            kafkaResources: overrides.kafka_resources,
            upgradeNotifications: Default::default(),
            region: region.name.clone(),
            environment: region.environment.to_string(),
            namespace: region.namespace.clone(),
            uid: Default::default(),
            secrets: Default::default(),
            state: Default::default(),
            workload: overrides.workload.unwrap_or_default(),
            prometheusAlerts: overrides.prometheus_alerts.unwrap_or_default(),
        })
    }
}

impl ManifestSource {
    pub fn build_simple(&self, conf: &Config, region: &Region) -> Result<SimpleManifest> {
        let base = self.build_base(conf)?;

        let overrides = self.overrides.clone();
        let defaults = overrides.defaults;
        let kong_apis = if let Some(k) = &region.kong {
            defaults.kong_apis.build(&KongApisBuildParams {
                service: base.name.to_string(),
                region: region.clone(),
                kong: k.clone(),
                single_api: defaults.kong,
            })?
        } else {
            // NB: this drops kong entries on the floor if region.kong is None
            vec![]
        };

        Ok(SimpleManifest {
            region: region.name.to_string(),

            enabled: !self.disabled && base.regions.contains(&region.name),
            external: self.external,

            // TODO: Make image non-optional
            image: Some(self.build_image(&base.name)?),
            version: overrides.version.build(&())?,
            kong_apis,
            base,
        })
    }

    pub fn build_base(&self, conf: &Config) -> Result<BaseManifest> {
        // TODO: Remove and use folder name
        let name = self.name.clone().require("name")?;
        let metadata = self.build_metadata(conf)?;
        let regions = self.regions.clone();

        Ok(BaseManifest {
            name,
            regions,
            metadata,
        })
    }

    fn build_image(&self, service: &str) -> Result<String> {
        if let Some(image) = &self.overrides.image {
            image.clone().build(&())
        } else if let Some(prefix) = &self.overrides.defaults.image_prefix {
            if prefix.ends_with('/') {
                bail!("image prefix must not end with a slash");
            }
            Ok(format!("{}/{}", prefix, service))
        } else {
            bail!("Image prefix is not defined")
        }
    }

    fn build_metadata(&self, conf: &Config) -> Result<Metadata> {
        let name = self.name.as_ref().expect("manifest name");
        let mut md = self.metadata.clone().require("metadata")?;

        if let Some(s) = conf.owners.squads.get(&md.team) {
            md.squad = Some(s.name.clone());
            md.tribe = conf
                .owners
                .tribes
                .values()
                .find(|t| t.squads.contains(&md.team))
                .map(|t| t.name.clone());
            if md.support.is_none() {
                md.support = s.slack.support.as_ref().map(Clone::clone);
            }
            if md.notifications.is_none() {
                md.notifications = s.slack.notifications.as_ref().map(Clone::clone);
            }
        } else {
            bail!(
                "{}: metadata.team '{}' must match a squad in teams.yml",
                name,
                md.team
            )
        }

        // teams.yml needs to have these specified
        if md.notifications.is_none() || md.support.is_none() {
            bail!("Need a notification and support channel for {}", md.team);
        }

        Ok(Metadata {
            repo: md.repo,
            team: md.team,
            context: md.context,
            squad: md.squad,
            tribe: md.tribe,
            language: md.language,
            gitTagTemplate: md.gitTagTemplate,
            contacts: md.contacts,
            maintainers: md.maintainers,
            support: md.support,
            notifications: md.notifications,
            runbook: md.runbook,
            description: md.description,
            docs: md.docs,
            ped: md.ped,
            testPlan: md.testPlan,
            releasePlan: md.releasePlan,
            threatModel: match md.threatModel {
                OneOrMany::One(x) => vec![x],
                OneOrMany::Many(xs) => xs,
            },
            dpsia: match md.dpsia {
                OneOrMany::One(x) => vec![x],
                OneOrMany::Many(xs) => xs,
            },
            custom: md.custom,
        })
    }

    // TODO: Extract DataHandlingSource
    fn build_data_handling(&self) -> Option<DataHandling> {
        let original = &self.overrides.data_handling;
        original.clone().map(|mut dh| {
            dh.implicits();
            dh
        })
    }

    // TODO: Extract KafkaSource
    fn build_kafka(&self, service: &str, reg: &Region) -> Option<Kafka> {
        let original = &self.overrides.kafka;
        original.clone().map(|mut kf| {
            kf.implicits(service, reg.clone());
            kf
        })
    }

    // TODO: Extract ConfigsSource
    async fn build_configs(&self, service: &str) -> Result<Option<ConfigMap>> {
        let original = &self.overrides.configs;
        if original.is_none() {
            return Ok(None);
        }
        let mut configs = original.clone().unwrap();
        for f in &mut configs.files {
            f.value = Some(read_template_file(service, &f.name).await?);
        }
        Ok(Some(configs))
    }

    pub(crate) fn merge_overrides(mut self, other: ManifestOverrides) -> Self {
        self.overrides = self.overrides.merge(other);
        self
    }
}

async fn read_template_file(svc: &str, tmpl: &str) -> Result<String> {
    use std::path::Path;
    use tokio::fs;
    // try to read file from ./services/{svc}/{tmpl} into `tpl` sting
    let pth = Path::new(".").join("services").join(svc).join(tmpl);
    let gpth = Path::new(".").join("templates").join(tmpl);
    let found_pth = if pth.exists() {
        debug!("Reading template in {}", pth.display());
        pth
    } else {
        if !gpth.exists() {
            bail!(
                "Template {} does not exist in neither {} nor {}",
                tmpl,
                pth.display(),
                gpth.display()
            );
        }
        debug!("Reading template in {}", gpth.display());
        gpth
    };
    // read the template - should work now
    let data = fs::read_to_string(&found_pth).await?;
    Ok(data)
}

impl ManifestDefaults {
    pub(crate) fn merge_source(self, mut other: ManifestSource) -> ManifestSource {
        other.overrides.defaults = self.merge(other.overrides.defaults);
        other
    }
}

#[cfg(test)]
mod tests {
    use merge::Merge;
    use std::collections::BTreeMap;

    use super::ManifestDefaults;

    #[test]
    fn merge() {
        let a = ManifestDefaults {
            image_prefix: Option::Some("alpha".into()),
            chart: Option::None,
            replica_count: Option::Some(1),
            env: {
                let mut env = BTreeMap::new();
                env.insert("a", "default-a");
                env.insert("b", "default-b");
                env.into()
            },
            ..Default::default()
        };
        let b = ManifestDefaults {
            image_prefix: Option::Some("beta".into()),
            chart: Option::Some("default".into()),
            replica_count: None,
            env: {
                let mut env = BTreeMap::new();
                env.insert("b", "override-b");
                env.insert("c", "override-c");
                env.into()
            },
            ..Default::default()
        };
        let merged = a.merge(b);
        assert_eq!(merged.image_prefix, Option::Some("beta".into()));
        assert_eq!(merged.chart, Option::Some("default".into()));
        assert_eq!(merged.replica_count, Option::Some(1));

        let mut expected_env = BTreeMap::new();
        expected_env.insert("a", "default-a");
        expected_env.insert("b", "override-b");
        expected_env.insert("c", "override-c");
        assert_eq!(merged.env, expected_env.into());
    }
}
