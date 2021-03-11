use super::{Config, Region, Result};
use semver::Version;
use shipcat_definitions::Environment;
/// This file contains the `shipcat get` subcommand
use std::collections::BTreeMap;

// ----------------------------------------------------------------------------
// Simple reducers

/// Find the hardcoded versions of services in a region
///
/// Services without a hardcoded version are not returned.
pub async fn versions(conf: &Config, region: &Region) -> Result<BTreeMap<String, Version>> {
    let mut output = BTreeMap::new();
    for mf in shipcat_filebacked::available(conf, region).await? {
        if let Some(v) = mf.version {
            if let Ok(sv) = Version::parse(&v) {
                output.insert(mf.base.name, sv);
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(output)
}

/// Find the hardcoded images of services in a region
///
/// Services without a hardcoded image will assume the shipcat.conf specific default
pub async fn images(conf: &Config, region: &Region) -> Result<BTreeMap<String, String>> {
    let mut output = BTreeMap::new();
    for mf in shipcat_filebacked::available(conf, region).await? {
        if let Some(i) = mf.image {
            output.insert(mf.base.name, i);
        }
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(output)
}

/// Generate codeowner strings for each service based based on team owners + admins
///
/// Cross references config.teams with manifest.metadata.team
/// Each returned string is Github CODEOWNER syntax
pub async fn codeowners(conf: &Config) -> Result<Vec<String>> {
    let mut output = vec![];
    let org = &conf.github.organisation;
    for mf in shipcat_filebacked::all(conf).await? {
        let md = mf.metadata;
        let mut ghids = vec![];

        if let Some(s) = conf.owners.squads.get(&md.team) {
            if let Some(gha) = &s.github.admins {
                ghids.push(format!("@{}/{}", org.to_lowercase(), gha));
            }
            // Add all squad members. Helpful because github codeowners are bad for teams
            // (Teams need to be added explicitly to the repo...)
            // Can perhaps be removed in the future
            for o in &s.members {
                if let Some(p) = conf.owners.people.get(o) {
                    if let Some(gh) = &p.github {
                        ghids.push(format!("@{}", gh));
                    }
                }
            }
        } else {
            warn!(
                "No squad found for {} in teams.yml - ignoring {}",
                md.team, mf.name
            );
        }

        if !ghids.is_empty() {
            output.push(format!("/services/{}/ {}", mf.name, ghids.join(" ")));
        }
    }
    println!("{}", output.join("\n"));
    Ok(output)
}

/// Generate vault policies based on team admins of services
///
/// Cross refereneces config.teams with manifest.metadata.team
/// The output is the same across all regions to avoid chicken-egg problems.
/// Introducing services to dev first, where dev vault section is open solves this.
///
/// Usage:
/// shipcat get vaultpolicy teamname | vault policy write github-team-name -
/// vault write auth/github/map/teams/github-team-name value=github-team-name
///
/// Assumes you have setup github provider using right organisation.
/// vault write auth/github/config organization={GithubOrganisation}
pub async fn vaultpolicy(conf: &Config, region: &Region, team_name: &str) -> Result<String> {
    let mfs = shipcat_filebacked::all(conf).await?;
    let team = if let Some(s) = conf.owners.squads.get(team_name) {
        if s.github.admins.is_none() {
            warn!(
                "Squad '{}' does not define a github.admins team in teams.yml",
                s.name
            );
        }
        s.name.clone()
    } else {
        bail!("Squad '{}' does not exist in teams.yml", team_name)
    };
    let output = region
        .vault
        .make_policy(mfs, &team, region.environment.clone())
        .await?;
    println!("{}", output);
    Ok(output)
}

// ----------------------------------------------------------------------------
// Reducers for the Config

#[derive(Serialize)]
pub struct ClusterInfo {
    pub region: String,
    pub namespace: String,
    pub environment: String,
    pub apiserver: String,
    pub cluster: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kong: Option<String>,
    // TODO: this optional
    pub vault: String,
}

/// Entry point for clusterinfo
///
/// Need explicit region: shipcat get -r preprodca-green clusterinfo
pub fn clusterinfo(conf: &Config, ctx: &str, cluster: Option<&str>) -> Result<ClusterInfo> {
    assert!(conf.has_all_regions()); // can't work with reduced configs
    let (clust, reg) = conf.resolve_cluster(ctx, cluster.map(String::from))?;
    let ci = ClusterInfo {
        region: reg.name,
        namespace: reg.namespace,
        environment: reg.environment.to_string(),
        apiserver: clust.api,
        cluster: clust.name,
        vault: reg.vault.url.clone(),
        kong: reg.kong.map(|k| k.config_url),
    };
    println!("{}", serde_json::to_string_pretty(&ci)?);
    Ok(ci)
}

/// Vault
///
/// Prints just the vault url for a region
/// Because this is invariant over a region
pub fn vault_url(region: &Region) -> Result<String> {
    let out = region.vault.url.clone();
    println!("{}", out);
    Ok(out)
}

// ----------------------------------------------------------------------------
// hybrid reducers

#[derive(Serialize)]
struct APIStatusOutput {
    region: RegionInfo,
    services: BTreeMap<String, APIServiceParams>,
}
#[derive(Serialize)]
struct APIServiceParams {
    hosts: String,
    uris: String,
    internal: bool,
    publiclyAccessible: bool,
    kompassPlugin: bool,
    websockets: bool,
}

#[derive(Serialize)]
struct RegionInfo {
    name: String,
    environment: Environment,
    base_urls: BTreeMap<String, String>,
    ip_whitelist: Vec<String>,
}
pub async fn apistatus(conf: &Config, reg: &Region) -> Result<()> {
    let mut services = BTreeMap::new();

    // Get Environment Config
    let region = RegionInfo {
        name: reg.name.clone(),
        environment: reg.environment.clone(),
        base_urls: reg.base_urls.clone(),
        ip_whitelist: reg.ip_whitelist.clone(),
    };

    // Get API Info from Manifests
    for svc in shipcat_filebacked::available(conf, reg).await? {
        let mf = shipcat_filebacked::load_manifest(&svc.base.name, &conf, &reg).await?;
        for k in mf.kongApis {
            let mut params = APIServiceParams {
                uris: k.uris.unwrap_or("".into()),
                hosts: k.hosts.join(","),
                internal: k.internal,
                publiclyAccessible: mf.publiclyAccessible,
                kompassPlugin: mf.kompass_plugin,
                websockets: false,
            };
            if let Some(g) = &mf.gate {
                // `manifest.verify` ensures that if there is a gate conf,
                // `gate.public` must be equal to `publiclyAccessible`.
                // That means that the following line does not alter the value
                // of `params.publiclyAccessible` but will be useful during the
                // migration of manifest configuration (ie deprecate
                // `publiclyAccessible` in favour of `gate.public`).
                params.publiclyAccessible = g.public;
                params.websockets = g.websockets;
            }
            services.insert(k.name, params);
        }
    }

    // Get extra API Info from Config: TODO: remove
    if let Some(kong) = &reg.kong {
        for (name, api) in kong.extra_apis.clone() {
            services.insert(name, APIServiceParams {
                uris: api.uris.unwrap_or("".into()),
                hosts: api.hosts.join(","),
                internal: api.internal,
                publiclyAccessible: api.publiclyAccessible,
                kompassPlugin: false,
                // TODO [DIP-499]: `extra_apis` do not support `gate` confs
                websockets: false,
            });
        }
    }

    let output = APIStatusOutput { region, services };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// ----------------------------------------------------------------------------
// Get Eventstreams and kafka reducers
use shipcat_definitions::structs::{kafkaresources, EventStream};

#[derive(Serialize)]
struct EventStreamsOutput {
    region: String,
    eventstreams: BTreeMap<String, EventStream>,
}

pub async fn eventstreams(conf: &Config, reg: &Region) -> Result<()> {
    let mut eventstreams = BTreeMap::new();

    // Get eventstream Info from Manifests
    for svc in shipcat_filebacked::available(conf, reg).await? {
        let mf = shipcat_filebacked::load_manifest(&svc.base.name, &conf, &reg).await?;
        for k in mf.eventStreams {
            eventstreams.insert(k.name.clone(), k);
        }
    }

    let region = reg.name.clone();
    let output = EventStreamsOutput { region, eventstreams };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// get Kafka Users
#[derive(Serialize)]
pub struct KafkaUsersInput {
    eventStreamsUsers: BTreeMap<String, EventStreamKafkaUsersParams>,
}

#[derive(Serialize)]
pub struct EventStreamKafkaUsersParams {
    service: String,
    producers: Vec<String>,
    consumers: Vec<String>,
}

#[derive(Serialize)]
pub struct KafkaResourceUserParams {
    service: String,
    acls: Vec<kafkaresources::AclDefinition>,
}

#[derive(Default, Serialize)]
struct EventStreamUsersOutput {
    service: String,
    produces_to: Vec<String>,
    consumes_from: Vec<String>,
}

#[derive(Default, Serialize)]
struct KafkaUsers {
    region: String,
    es_kafka_users: BTreeMap<String, EventStreamUsersOutput>,
    kr_kafka_users: BTreeMap<String, KafkaResourceUserParams>,
}

fn transformEventstreamUsers(input: KafkaUsersInput) -> BTreeMap<String, EventStreamUsersOutput> {
    let mut output: BTreeMap<String, EventStreamUsersOutput> = BTreeMap::new();

    for (name, eventStreamsUsers) in input.eventStreamsUsers {
        for user in eventStreamsUsers.producers {
            output.entry(user).or_default().produces_to.push(name.clone());
        }
        for user in eventStreamsUsers.consumers {
            output.entry(user).or_default().consumes_from.push(name.clone());
        }
    }

    output
}

pub async fn kafkausers(conf: &Config, reg: &Region) -> Result<()> {
    let mut eventStreamsUsers = BTreeMap::new();
    let mut krusers = BTreeMap::new();

    // Get kafka users from eventstreams struct
    for svc in shipcat_filebacked::available(conf, reg).await? {
        let mf = shipcat_filebacked::load_manifest(&svc.base.name, &conf, &reg).await?;
        for k in mf.eventStreams {
            let params = EventStreamKafkaUsersParams {
                service: String::from(&svc.base.name),
                producers: k.producers,
                consumers: k.consumers,
            };
            eventStreamsUsers.insert(k.name, params);
        }
        // get kafka users from KafkaResources struct
        if let Some(kr) = mf.kafkaResources {
            for user in kr.users {
                let params = KafkaResourceUserParams {
                    service: String::from(&svc.base.name),
                    acls: user.acls,
                };
                krusers.insert(user.name, params);
            }
        }
    }
    let region = reg.name.clone();
    let output = KafkaUsers {
        region,
        es_kafka_users: transformEventstreamUsers(KafkaUsersInput { eventStreamsUsers }),
        kr_kafka_users: krusers,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// get kafka topics
#[derive(Default, Serialize)]
struct KafkaTopics {
    region: String,
    kafkaTopics: BTreeMap<String, KafkaTopicParams>,
}

#[derive(Serialize)]
pub struct KafkaTopicParams {
    service: String,
    topicType: String,
    partitions: String,
    replicas: String,
    config: BTreeMap<String, String>,
}

pub async fn kafkatopics(conf: &Config, reg: &Region) -> Result<()> {
    let mut kafkaTopics = BTreeMap::new();

    // Get eventstream Info from Manifests
    for svc in shipcat_filebacked::available(conf, reg).await? {
        let mf = shipcat_filebacked::load_manifest(&svc.base.name, &conf, &reg).await?;

        // get kafka topics from eventstream struct
        for topic in mf.eventStreams {
            let params = KafkaTopicParams {
                service: String::from(&svc.base.name),
                topicType: "EventStream".to_string(),
                partitions: topic
                    .config
                    .get("partitions")
                    .map(String::from)
                    .unwrap_or_default(),
                replicas: topic.config.get("replicas").map(String::from).unwrap_or_default(),
                config: topic.config,
            };
            kafkaTopics.insert(topic.name, params);
        }
        // get kafka topics from KafkaResources struct
        if let Some(kr) = mf.kafkaResources {
            for topic in kr.topics {
                let params = KafkaTopicParams {
                    service: String::from(&svc.base.name),
                    topicType: "KafkaResource".to_string(),
                    partitions: topic.partitions.to_string(),
                    replicas: topic.replicas.to_string(),
                    config: topic.config,
                };
                kafkaTopics.insert(topic.name, params);
            }
        }
    }
    let region = reg.name.clone();
    let output = KafkaTopics { region, kafkaTopics };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
