use super::{Config, Error, Manifest, Region, Result};
use futures::stream::{self, StreamExt};
use shipcat_definitions::{math::ResourceTotals, BaseManifest};
use std::{collections::BTreeMap, str::FromStr};

use generic_array::{typenum::U4, GenericArray};
use size_format::{PointSeparated, PrefixType, SizeFormatter, SizeFormatterBinary};

// Nice size formatting of millicores.
struct Millicores;

// milli is default unit, then stop sub-dividing.
impl PrefixType for Millicores {
    type N = U4;

    const PREFIX_SIZE: u32 = 1000;

    fn prefixes() -> GenericArray<&'static str, Self::N> {
        ["m", "", "k", "M"].into()
    }
}

/// What to sort resources by (for main)
pub enum ResourceOrder {
    Cpu,
    Memory,
}

impl FromStr for ResourceOrder {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
        match input {
            "cpu" => Ok(ResourceOrder::Cpu),
            "memory" => Ok(ResourceOrder::Memory),
            _ => bail!("Resource type must be cpu or memory"),
        }
    }
}

async fn load_mf_req(svc: String, conf: &Config, reg: &Region) -> Result<(Manifest, ResourceTotals)> {
    let mf = shipcat_filebacked::load_manifest(&svc, &conf, &reg)
        .await?
        .stub(&reg)
        .await?;
    let res = mf.compute_resource_totals()?;
    Ok((mf, res))
}

async fn calculate_manifest_requests(conf: &Config, reg: &Region) -> Result<Vec<(Manifest, ResourceTotals)>> {
    let available = shipcat_filebacked::available(conf, &reg).await?;
    let mut buffered = stream::iter(available)
        .map(move |mf| load_mf_req(mf.base.name, conf, reg))
        .buffer_unordered(100);
    let mut mfs = vec![];
    while let Some(r) = buffered.next().await {
        mfs.push(r?);
    }
    Ok(mfs)
}

async fn load_mf_req_world(base: BaseManifest, conf: &Config) -> Result<Option<(Manifest, ResourceTotals)>> {
    let mut res = ResourceTotals::default();
    let mut first_mf = None;
    debug!("{} looping over {:?}", base.name, base.regions);
    for r in &base.regions {
        if let Some(reg) = conf.get_region_unchecked(&r) {
            trace!("valid region: {}", reg.name);
            let mf = shipcat_filebacked::load_manifest(&base.name, &conf, &reg)
                .await?
                .stub(&reg)
                .await?;
            if !mf.disabled && !mf.external {
                let ResourceTotals { base: rb, extra: se } = mf.compute_resource_totals()?;
                debug!(
                    "{} in {}: adding reqs: {} {}",
                    mf.name, r, rb.requests.cpu, rb.requests.memory
                );
                res.base += rb.clone();
                res.extra += se.clone();
                first_mf = Some(mf);
            }
        }
    }
    if let Some(mf) = first_mf {
        Ok(Some((mf, res)))
    } else {
        Ok(None)
    }
}

async fn calculate_manifest_requests_world(conf: &Config) -> Result<Vec<(Manifest, ResourceTotals)>> {
    let all = shipcat_filebacked::all(conf).await?;
    let mut buffered = stream::iter(all)
        .map(|mf| load_mf_req_world(mf, conf))
        .buffer_unordered(100);
    let mut mfs = vec![];
    while let Some(r) = buffered.next().await {
        if let Some(v) = r? {
            mfs.push(v);
        }
    }
    Ok(mfs)
}

/// Resource top for a every region
///
/// This presents an analytical solution to aggregate resource requests.
/// It does NOT talk to kubernetes.
///
/// It works out ResourceTotals based on Manifest properties analytically.
pub async fn world_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
) -> Result<Vec<(Manifest, ResourceTotals)>> {
    let mfs = calculate_manifest_requests_world(conf).await?;
    let mfs = sort_and_print_resources(mfs, order, fmt, ub)?;
    Ok(mfs)
}

/// Resource top for a single region
///
/// This presents an analytical solution to aggregate resource requests in a region.
/// It does NOT talk to kubernetes.
///
/// It works out ResourceTotals based on Manifest properties analytically.
pub async fn region_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
    reg: &Region,
) -> Result<Vec<(Manifest, ResourceTotals)>> {
    let mfs = calculate_manifest_requests(conf, reg).await?;
    let mfs = sort_and_print_resources(mfs, order, fmt, ub)?;
    Ok(mfs)
}

/// How to format numbers
pub enum OutputFormat {
    /// Human readable table using size-formatter
    Table,
    /// Yaml output with raw numbers in milli-cores and Bytes
    Yaml,
}

impl FromStr for OutputFormat {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
        match input {
            "table" => Ok(Self::Table),
            "yaml" => Ok(Self::Yaml),
            _ => bail!("Resource type must be table or yaml"),
        }
    }
}
impl Default for OutputFormat {
    fn default() -> Self {
        Self::Table
    }
}

fn sort_and_print_resources(
    mut mfs: Vec<(Manifest, ResourceTotals)>,
    order: ResourceOrder,
    formatting: OutputFormat,
    upper_bounds: bool,
) -> Result<Vec<(Manifest, ResourceTotals)>> {
    match order {
        ResourceOrder::Cpu => {
            mfs.sort_by(|(_, r1), (_, r2)| {
                if upper_bounds {
                    (r2.base.requests.cpu + r2.extra.requests.cpu)
                        .partial_cmp(&(r1.base.requests.cpu + r1.extra.requests.cpu))
                        .unwrap()
                } else {
                    r2.base.requests.cpu.partial_cmp(&r1.base.requests.cpu).unwrap()
                }
            });
        }
        ResourceOrder::Memory => {
            mfs.sort_by(|(_, r1), (_, r2)| {
                if upper_bounds {
                    (r2.base.requests.memory + r2.extra.requests.memory)
                        .partial_cmp(&(r1.base.requests.memory + r1.extra.requests.memory))
                        .unwrap()
                } else {
                    r2.base
                        .requests
                        .memory
                        .partial_cmp(&r1.base.requests.memory)
                        .unwrap()
                }
            });
        }
    }
    // Convert the sorted data into a printable structure.
    #[derive(Serialize)]
    struct YamlOutput {
        name: String,
        squad: String,
        tribe: Option<String>,
        cpu: u64,
        memory: u64,
    }
    let output = mfs
        .iter()
        .map(|(mf, r)| {
            // Convert to Millicores and Bytes
            let (cpu, memory) = if upper_bounds {
                let ub_cpu = (1000.0 * (r.base.requests.cpu + r.extra.requests.cpu)) as u64;
                let ub_memory = (r.base.requests.memory + r.extra.requests.memory) as u64;
                (ub_cpu, ub_memory)
            } else {
                let lb_cpu = (1000.0 * r.base.requests.cpu) as u64;
                let lb_memory = r.base.requests.memory as u64;
                (lb_cpu, lb_memory)
            };
            YamlOutput {
                memory,
                cpu,
                name: mf.name.clone(),
                squad: mf.metadata.as_ref().unwrap().team.clone(),
                tribe: mf.metadata.as_ref().unwrap().tribe.clone(),
            }
        })
        .collect::<Vec<_>>();

    match formatting {
        OutputFormat::Table => {
            println!(
                "{0:<50} {1:<8} {2:<8} {3:40} {4:40}",
                "SERVICE", "CPU", "MEMORY", "SQUAD", "TRIBE"
            );
            output.into_iter().for_each(|o| {
                println!(
                    "{0:<50} {1:width$} {2:width$} {3:<40} {4:<40}",
                    o.name,
                    format!(
                        "{:.0}",
                        SizeFormatter::<u64, Millicores, PointSeparated>::new(o.cpu)
                    ),
                    format!("{:.0}", SizeFormatterBinary::new(o.memory)),
                    o.squad,
                    o.tribe.unwrap_or("".to_string()),
                    width = 8,
                );
            });
        }
        OutputFormat::Yaml => {
            println!("{}", serde_yaml::to_string(&output)?);
        }
    }
    Ok(mfs)
}

fn fold_manifests_by_squad(reqs: Vec<(Manifest, ResourceTotals)>) -> Result<Vec<(String, ResourceTotals)>> {
    let team_requests: Vec<(String, ResourceTotals)> = reqs
        .into_iter()
        .fold(BTreeMap::<String, ResourceTotals>::new(), |mut acc, (mf, res)| {
            acc.entry(mf.metadata.as_ref().unwrap().squad.clone().unwrap())
                .and_modify(|e| {
                    let ResourceTotals { base: rb, extra: se } = &res;
                    e.base += rb.clone();
                    e.extra += se.clone();
                })
                .or_insert(res);
            acc
        })
        .into_iter()
        .map(|(t, res)| (t, res)) // btreemap -> vector
        .collect();
    Ok(team_requests)
}

fn fold_manifests_by_tribe(reqs: Vec<(Manifest, ResourceTotals)>) -> Result<Vec<(String, ResourceTotals)>> {
    let team_requests: Vec<(String, ResourceTotals)> = reqs
        .into_iter()
        .fold(BTreeMap::<String, ResourceTotals>::new(), |mut acc, (mf, res)| {
            let md = mf.metadata.as_ref().unwrap();
            if let Some(tribe) = &md.tribe {
                acc.entry(tribe.to_string())
                    .and_modify(|e| {
                        let ResourceTotals { base: rb, extra: se } = &res;
                        e.base += rb.clone();
                        e.extra += se.clone();
                    })
                    .or_insert(res);
            } else {
                // Can happen if ewok orphaned_squads is not set to hard error
                warn!("Could not find a matching tribe for {}", mf.name);
            }
            acc
        })
        .into_iter()
        .map(|(t, res)| (t, res)) // btreemap -> vector
        .collect();
    Ok(team_requests)
}

/// Resource squad top for a single region
///
/// Same data as region_requests, but aggregated across squads
pub async fn region_squad_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
    reg: &Region,
) -> Result<Vec<(String, ResourceTotals)>> {
    let mfs = calculate_manifest_requests(conf, reg).await?;
    let team_requests = fold_manifests_by_squad(mfs)?;
    let sorted = sort_and_print_team_resources(team_requests, "squad", order, fmt, ub)?;
    Ok(sorted)
}

/// Resource tribe top for a single region
///
/// Uses same data as reguion_requests but aggregates across tribes.
/// If tribes exists for all squads, then the data sums up to the same numbers
pub async fn region_tribe_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
    reg: &Region,
) -> Result<Vec<(String, ResourceTotals)>> {
    let mfs = calculate_manifest_requests(conf, reg).await?;
    let team_requests = fold_manifests_by_tribe(mfs)?;
    let sorted = sort_and_print_team_resources(team_requests, "tribe", order, fmt, ub)?;
    Ok(sorted)
}

/// Resource squad top for every region
///
/// Same data as world_requests, but aggregated across squads
pub async fn world_squad_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
) -> Result<Vec<(String, ResourceTotals)>> {
    let mfs = calculate_manifest_requests_world(conf).await?;
    let team_requests = fold_manifests_by_squad(mfs)?;
    let sorted = sort_and_print_team_resources(team_requests, "squad", order, fmt, ub)?;
    Ok(sorted)
}

/// Resource tribe top for every region
///
/// Uses same data as world_requests but aggregates across tribes.
/// If tribes exists for all squads, then the data sums up to the same numbers
pub async fn world_tribe_requests(
    order: ResourceOrder,
    ub: bool,
    fmt: OutputFormat,
    conf: &Config,
) -> Result<Vec<(String, ResourceTotals)>> {
    let mfs = calculate_manifest_requests_world(conf).await?;
    let team_requests = fold_manifests_by_tribe(mfs)?;
    let sorted = sort_and_print_team_resources(team_requests, "tribe", order, fmt, ub)?;
    Ok(sorted)
}

fn sort_and_print_team_resources(
    mut reqs: Vec<(String, ResourceTotals)>,
    team_type: &str,
    order: ResourceOrder,
    formatting: OutputFormat,
    upper_bounds: bool,
) -> Result<Vec<(String, ResourceTotals)>> {
    match order {
        ResourceOrder::Cpu => {
            reqs.sort_by(|(_, r1), (_, r2)| {
                if upper_bounds {
                    (r2.base.requests.cpu + r2.extra.requests.cpu)
                        .partial_cmp(&(r1.base.requests.cpu + r1.extra.requests.cpu))
                        .unwrap()
                } else {
                    r2.base.requests.cpu.partial_cmp(&r1.base.requests.cpu).unwrap()
                }
            });
        }
        ResourceOrder::Memory => {
            reqs.sort_by(|(_, r1), (_, r2)| {
                if upper_bounds {
                    (r2.base.requests.memory + r2.extra.requests.memory)
                        .partial_cmp(&(r1.base.requests.memory + r1.extra.requests.memory))
                        .unwrap()
                } else {
                    r2.base
                        .requests
                        .memory
                        .partial_cmp(&r1.base.requests.memory)
                        .unwrap()
                }
            });
        }
    }
    // Convert the sorted data into a printable structure.
    #[derive(Serialize)]
    struct YamlOutput {
        team: String,
        cpu: u64,
        memory: u64,
    }
    let output = reqs
        .iter()
        .map(|(team, r)| {
            // Convert to Millicores and Bytes
            let (cpu, memory) = if upper_bounds {
                let ub_cpu = (1000.0 * (r.base.requests.cpu + r.extra.requests.cpu)) as u64;
                let ub_memory = (r.base.requests.memory + r.extra.requests.memory) as u64;
                (ub_cpu, ub_memory)
            } else {
                let lb_cpu = (1000.0 * r.base.requests.cpu) as u64;
                let lb_memory = r.base.requests.memory as u64;
                (lb_cpu, lb_memory)
            };
            YamlOutput {
                memory,
                cpu,
                team: team.to_string(),
            }
        })
        .collect::<Vec<_>>();

    match formatting {
        OutputFormat::Table => {
            println!("{0:<45} {1:<8} {2:<8}", team_type.to_uppercase(), "CPU", "MEMORY");
            output.into_iter().for_each(|o| {
                println!(
                    "{0:<45} {1:width$} {2:width$}",
                    o.team,
                    format!(
                        "{:.0}",
                        SizeFormatter::<u64, Millicores, PointSeparated>::new(o.cpu)
                    ),
                    format!("{:.0}", SizeFormatterBinary::new(o.memory)),
                    width = 8,
                );
            });
        }
        OutputFormat::Yaml => {
            println!("{}", serde_yaml::to_string(&output)?);
        }
    }
    Ok(reqs)
}
