#![allow(non_snake_case)]

/// Verify traits sometimes need to cross reference stuff from other manifests
pub use super::Manifest;
/// Verify trait gets the Region and Team
pub use super::Region;
/// Allow normal error handling from structs
pub use super::{ErrorKind, Result, ResultExt};

// Structs that exist in the manifest

mod dependency;
pub use self::dependency::{Dependency, DependencyProtocol};

/// DestinationRule struct
mod destinationrule;
pub use self::destinationrule::DestinationRule;

mod worker;
pub use self::worker::Worker;

/// Kong configs
pub mod kong;
pub use self::kong::{Authentication, BabylonAuthHeader, Cors, Kong, KongRateLimit};

pub mod authorization;
pub use self::authorization::Authorization;

/// Gate configs
pub mod gate;
pub use self::gate::Gate;

/// Kongfig configs
pub mod kongfig;
pub use self::kongfig::{Api, Certificate, Consumer, Plugin, Upstream};

/// Kafka configs
pub mod kafka;
pub use self::kafka::Kafka;

// Kubernetes - first are abstractions latter ones are straight translations

// abstractions - these have special handling
/// Templated configmap abstractions
mod configmap;
pub use self::configmap::{ConfigMap, ConfigMappedFile};
/// Healthcheck abstraction
mod healthcheck;
pub use self::healthcheck::HealthCheck;

mod env;
pub use self::env::EnvVars;

// translations - these are typically inlined in templates as yaml
/// Kubernetes resource structs
pub mod resources;
pub use self::resources::{parse_memory, ResourceRequirements};
/// Kubernetes volumes
pub mod volume;
pub use self::volume::{Volume, VolumeMount};
/// Kubernetes host aliases
mod hostalias;
pub use self::hostalias::HostAlias;
/// Kubernetes health check probes
mod probes;
pub use self::probes::Probe;
/// Kubernetes rolling-update settings
pub mod rollingupdate;
pub use self::rollingupdate::RollingUpdate;
/// Kubernetes horizontal pod autoscaler
pub mod autoscaling;
/// Kubernetes container lifecycle events
mod lifecycle;
/// Kuberneter tolerations
pub mod tolerations;
pub use self::lifecycle::{LifeCycle, LifeCycleHandler};

pub mod metadata;
pub use self::metadata::{Contact, Metadata, SlackChannel};

/// Security related structs
pub mod security;

mod securitycontext;
pub use securitycontext::SecurityContext;

mod vault;
pub use self::vault::VaultOpts;

/// Cron Jobs
pub mod cronjob;
pub use self::cronjob::{CronJob, JobVolumeClaim};

// Kubernetes Containers
pub mod container;
pub use self::container::Container;

pub mod port;
pub use self::port::Port;

/// Rbac
pub mod rbac;
pub use self::rbac::Rbac;

// PersistentVolume
mod persistentvolume;
pub use self::persistentvolume::PersistentVolume;

pub mod newrelic;

pub mod sentry;

mod notifications;
pub use notifications::NotificationMode;

// EventStreams / Kafka related struct
mod eventstream;
pub use self::eventstream::EventStream;

pub mod kafkaresources;
pub use self::kafkaresources::KafkaResources;

pub mod prometheusalert;
pub use self::prometheusalert::PrometheusAlert;
