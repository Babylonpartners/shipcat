use futures::stream::{self, StreamExt};
use shipcat_definitions::{BaseManifest, Config, Region, ShipcatConfig};
use shipcat_filebacked::SimpleManifest;

use super::{kubectl, Error, ErrorKind, Result};
use crate::{
    apply, diff, helm,
    kubeapi::ShipKube,
    webhooks::{self, UpgradeState},
};

struct DiffResult {
    name: String,
    diff: Option<String>,
}
async fn diff_summary(svc: String, conf: &Config, reg: &Region) -> Result<DiffResult> {
    let mut mf = shipcat_filebacked::load_manifest(&svc, &conf, &reg)
        .await?
        .complete(&reg)
        .await?;
    // complete with version and uid from crd
    let s = ShipKube::new(&mf).await?;
    let crd = s.get().await?;
    mf.version = mf.version.or(crd.spec.version);
    mf.uid = crd.metadata.uid;
    info!("diffing {}", mf.name);
    let d = if let Some(kdiffunobfusc) = diff::template_vs_kubectl(&mf).await? {
        let kubediff = diff::obfuscate_secrets(
            kdiffunobfusc, // move this away quickly..
            mf.get_secrets(),
        );
        let smalldiff = diff::minify(&kubediff);
        Some(smalldiff)
    } else {
        None
    };
    Ok(DiffResult {
        name: mf.name,
        diff: d,
    })
}

/// Diffs all services in a region
///
/// Helper that shells out to kubectl diff in parallel.
pub async fn mass_diff(conf: &Config, reg: &Region) -> Result<()> {
    let svcs = shipcat_filebacked::available(conf, reg).await?;
    assert!(conf.has_secrets());

    let mut buffered = stream::iter(svcs)
        .map(move |mf| diff_summary(mf.base.name, &conf, &reg))
        .buffer_unordered(10);

    let mut errs = vec![];
    let mut diffs = vec![];
    while let Some(r) = buffered.next().await {
        match r {
            Ok(dr) => diffs.push(dr),
            Err(e) => errs.push(e),
        }
    }
    for dr in diffs {
        if let Some(diff) = dr.diff {
            info!("{} diff output:\n{}", dr.name, diff);
        } else {
            info!("{} unchanged", dr.name)
        }
    }
    if !errs.is_empty() {
        for e in &errs {
            match e {
                Error(ErrorKind::KubeError(e2), _) => {
                    warn!("{}", e2); // probably missing service (undiffeable)
                }
                Error(ErrorKind::MissingRollingVersion(svc), _) => {
                    // This only happens in rolling envs because version is mandatory in other envs
                    warn!("ignored missing service {}: {}", svc, e.description());
                }
                _ => {
                    error!("{}", e);
                    debug!("{:?}", e);
                }
            }
        }
        bail!("Failed to diff {} manifests", errs.len());
    }
    Ok(())
}

async fn check_summary(svc: String, skipped: &[String], conf: &Config, reg: &Region) -> Result<String> {
    let mut mf = shipcat_filebacked::load_manifest(&svc, &conf, &reg)
        .await?
        .stub(&reg)
        .await?;
    mf.version = mf.version.or(Some("latest".to_string()));
    mf.uid = Some("FAKE-GUID".to_string());

    info!("verifying template for {}", mf.name);
    let tpl = helm::template(&mf, None).await?;
    helm::template_check(&mf, reg, skipped, &tpl)?;
    Ok(mf.name)
}

/// Verifies all populated templates for all services in a region
///
/// Helper that shells out to helm template in parallel.
pub async fn mass_template_verify(conf: &Config, reg: &Region, skipped: &[String]) -> Result<()> {
    let svcs = shipcat_filebacked::available(conf, reg).await?;

    let mut buffered = stream::iter(svcs)
        .map(move |mf| check_summary(mf.base.name, &skipped, &conf, &reg))
        .buffer_unordered(100);

    let (mut errs, mut passed): (Vec<Error>, Vec<_>) = (vec![], vec![]);
    while let Some(r) = buffered.next().await {
        match r {
            Ok(p) => passed.push(p),
            Err(e) => errs.push(e),
        }
    }

    for n in passed {
        info!("{} verified", n)
    }
    if !errs.is_empty() {
        for e in &errs {
            error!("{}", e);
            debug!("{:?}", e);
        }
        bail!("Failed to verify templates for {} manifest", errs.len());
    }
    Ok(())
}

/// Apply CRDs in all region
pub async fn crd_install(reg: &Region) -> Result<()> {
    use shipcat_definitions::gen_all_crds;
    for crdef in gen_all_crds() {
        kubectl::apply_resource(&reg.name, crdef, &reg.namespace).await?;
    }
    Ok(())
}

/// Apply all services in the region
///
/// Helper that shells out to kubectl apply in parallel.
pub async fn mass_crd(conf_sec: &Config, conf_base: &Config, reg: &Region, n_workers: usize) -> Result<()> {
    let svcs = shipcat_filebacked::available(conf_base, reg).await?;
    crd_reconcile(svcs, conf_sec, conf_base, &reg.name, n_workers).await
}

async fn crd_reconcile(
    svcs: Vec<SimpleManifest>,
    config_sec: &Config,
    config_base: &Config,
    region: &str,
    n_workers: usize,
) -> Result<()> {
    // NB: This needs config_base for base crd application
    // shipcatconfig crd should not have secrets when applied
    // shipcatmanifest_crd should not have secrets when applied (fine as long as manifest is not complete())
    // but when doing the actual upgrade we need a config + region with secrets.
    assert!(config_sec.has_secrets());
    assert!(!config_base.has_secrets());
    let region_sec = config_sec
        .get_regions()
        .iter()
        .find(|r| r.name == region)
        .unwrap()
        .clone();
    let region_base = config_base
        .get_regions()
        .iter()
        .find(|r| r.name == region)
        .unwrap()
        .clone();

    webhooks::reconcile_event(UpgradeState::Pending, &region_sec).await;
    // Always reconcile the CRDs (definitions themselves) first
    crd_install(&region_base).await?;

    // Make sure config can apply first
    let applycfg: ShipcatConfig = if let Some(ref crs) = &region_base.customResources {
        // special configtype detected - re-populating config object
        Config::new(crs.shipcatConfig.clone(), &region_base.name).await?.0
    } else {
        config_base.clone()
    }
    .into();
    kubectl::apply_resource(&region_base.name, applycfg, &region_base.namespace).await?;

    // Single instruction kubectl delete shipcat manifests .... of excess ones
    let svc_names = svcs.iter().map(|x| x.base.name.to_string()).collect::<Vec<_>>();
    let excess = kubectl::find_redundant_manifests(&region_sec.namespace, &svc_names).await?;
    if !excess.is_empty() {
        info!("Will remove excess manifests: {:?}", excess);
    }
    for svc in excess {
        // NB: doing deletion sequentially...
        apply::delete(&svc, &region_sec, &config_sec).await?;
    }

    info!(
        "Spawning {} parallel kube jobs with {} workers",
        svcs.len(),
        n_workers
    );

    webhooks::reconcile_event(UpgradeState::Started, &region_sec).await;
    // then parallel apply the remaining ones
    let force = std::env::var("SHIPCAT_MASS_RECONCILE").unwrap_or("0".into()) == "1";
    let wait_for_rollout = true;

    let conf = config_sec.clone();
    let reg = region_sec.clone();
    let mut buffered = stream::iter(svcs)
        .map(|mf| {
            debug!("Running CRD reconcile for {:?}", mf.base.name);
            apply::apply(mf.base.name, force, &reg, &conf, wait_for_rollout, None)
        })
        .buffer_unordered(n_workers);

    let mut errs = vec![];
    while let Some(r) = buffered.next().await {
        if let Err(e) = r {
            warn!("{}", e);
            errs.push(e);
        }
    }

    // propagate first non-ignorable error if exists
    for e in errs {
        match e {
            Error(ErrorKind::MissingRollingVersion(svc), _) => {
                // This only happens in rolling envs because version is mandatory in other envs
                warn!(
                    "'{}' missing version for {} - please add or install",
                    svc, region_sec.name
                );
            }
            // remaining cases not ignorable
            _ => {
                webhooks::reconcile_event(UpgradeState::Failed, &region_sec).await;
                return Err(e);
            }
        }
    }

    // Otherwise we're good
    webhooks::reconcile_event(UpgradeState::Completed, &region_sec).await;
    Ok(())
}

/// Apply all vault policies in a region
///
/// Generates and writes policies direct to vault using their github team name as auth mappers.
/// Equivalent to:
///
/// ```pseudo
/// for team in config.teams:
///   shipcat get vaultpolicy {team.name} | vault policy write {team.admins} -
///   vault write auth/github/map/teams/{team.admins} value={team.admins}
/// ```
///
/// using vault setup for the vault specified in the `Region`.
/// If one vault is reused for all regions, this can be done once.
///
/// Requires a `vault login` outside of this command as a user who
/// is sufficiently elevated to write general policies.
pub async fn mass_vault(conf: &Config, reg: &Region, n_workers: usize) -> Result<()> {
    let svcs = shipcat_filebacked::all(conf).await?;
    vault_reconcile(svcs, conf, reg, n_workers).await
}

async fn vault_reconcile(
    mfs: Vec<BaseManifest>,
    conf: &Config,
    region: &Region,
    n_workers: usize,
) -> Result<()> {
    let n_jobs = conf.owners.squads.len();
    info!(
        "Starting {} parallel vault jobs with {} workers",
        n_jobs, n_workers
    );

    // then parallel apply the remaining ones
    let reg = region.clone();

    let mut buffered = stream::iter(conf.owners.clone().squads)
        .map(|(name, squad)| {
            let mfs = mfs.clone();
            debug!("Running vault reconcile for {}", name);
            vault_reconcile_worker(mfs, name, squad.github.admins, &reg)
        })
        .buffer_unordered(n_workers);

    let mut errs = vec![];
    while let Some(r) = buffered.next().await {
        if let Err(e) = r {
            warn!("{}", e);
            errs.push(e);
        }
    }

    // propagate first non-ignorable error if exists
    if let Some(e) = errs.into_iter().next() {
        // no errors ignoreable atm
        return Err(e);
    }
    Ok(())
}

async fn vault_reconcile_worker(
    svcs: Vec<BaseManifest>,
    team: String,
    admins_opt: Option<String>,
    reg: &Region,
) -> Result<()> {
    use std::{fs::File, io::Write, path::Path};
    if admins_opt.is_none() {
        debug!("'{}' does not have a github admins team - ignoring", team);
        return Ok(()); // nothing to do
    };
    let admins = admins_opt.unwrap();

    let policy = reg
        .vault
        .make_policy(svcs, &team, reg.environment.clone())
        .await?;
    debug!("Vault policy: {}", policy);
    // Write policy to a file named "{admins}-policy.hcl"
    let pth = Path::new(".").join(format!("{}-policy.hcl", admins));
    info!("Writing vault policy for {} to {}", admins, pth.display());
    let mut f = File::create(&pth)?;
    writeln!(f, "{}", policy)?;
    // Write a vault policy with the name equal to the admin team:
    use tokio::process::Command;
    // vault write policy < file
    {
        info!("Applying vault policy for {} in {}", admins, reg.name);
        let write_args = vec![
            "policy".into(),
            "write".into(),
            admins.clone(),
            format!("{}-policy.hcl", admins),
        ];
        debug!("vault {}", write_args.join(" "));
        let s = Command::new("vault").args(&write_args).status().await?;
        if !s.success() {
            bail!("Subprocess failure from vault: {}", s.code().unwrap_or(1001))
        }
    }
    // vault write auth -> team
    {
        info!(
            "Associating vault policy for {} with github team {} in {}",
            team, admins, reg.name
        );
        let assoc_args = vec![
            "write".into(),
            format!("auth/github/map/teams/{}", admins),
            format!("value={}", admins),
        ];
        debug!("vault {}", assoc_args.join(" "));
        let s = Command::new("vault").args(&assoc_args).status().await?;
        if !s.success() {
            bail!("Subprocess failure from vault: {}", s.code().unwrap_or(1001))
        }
    }
    Ok(())
}
