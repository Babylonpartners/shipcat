//- kubeapi module to track upgrades
use crate::{kubeapi::ShipKube, slack::short_ver, Result};
use chrono::{Duration, Utc};
use k8s_openapi::api::{
    apps::v1::{Deployment, ReplicaSet, StatefulSet},
    core::v1::Pod,
};
use kube::api::{Meta, ObjectList};
use shipcat_definitions::{Manifest, PrimaryWorkload};
use std::{
    convert::{TryFrom, TryInto},
    fmt::{self, Debug},
};

fn format_duration(dur: Duration) -> String {
    let days = dur.num_days();
    let hours = dur.num_hours();
    let mins = dur.num_minutes();
    if days > 0 {
        format!("{}d", days)
    } else if hours > 0 {
        format!("{}h", hours)
    } else {
        format!("{}m", mins)
    }
}

/// A summary of a Pod's status
pub struct PodSummary {
    pub name: String,
    pub age: Duration,
    pub phase: String,
    pub running: i32,
    pub containers: u32,
    pub restarts: i32,
    pub version: String,
}

impl Debug for PodSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NB: this format string is a generic one used by shipcat status
        write!(
            f,
            "{0:<60} {1:<8} {2:<12} {3:<6} {4:<8} {5:<12}",
            self.name,
            self.version,
            self.phase,
            format!("{}/{}", self.running, self.containers),
            self.restarts,
            format_duration(self.age)
        )
    }
}

impl TryFrom<Pod> for PodSummary {
    type Error = crate::Error;

    /// Helper to convert the openapi Pod to the useful info
    fn try_from(pod: Pod) -> Result<PodSummary> {
        let mut name = "unknown name";
        let mut age = Duration::seconds(0);
        let mut phase = "unknown phase".to_string();
        let mut version = "unknown ver".to_string();

        if let Some(meta) = &pod.metadata {
            name = match &meta.name {
                Some(n) => n,
                None => bail!("missing metadata.name on pod {:?}", pod),
            };
            let ts = match &meta.creation_timestamp {
                Some(t) => t.0,
                None => bail!("missing metadata.creation_timestamp on pod {}", name),
            };
            age = Utc::now().signed_duration_since(ts);
        }

        let mut running = 0;
        let mut containers = 0;
        let mut restarts = 0;
        if let Some(status) = pod.status {
            phase = match status.phase {
                Some(p) => p,
                None => bail!("missing status.phase on pod {}", name),
            };
            for s in status.container_statuses.unwrap_or_default() {
                running += if s.ready { 1 } else { 0 };
                containers += 1;
                restarts = std::cmp::max(restarts, s.restart_count);
            }
        }
        if let Some(spec) = pod.spec {
            let main_container = &spec.containers[0];
            version = short_ver(
                main_container
                    .image
                    .as_ref()
                    .unwrap()
                    .split(':')
                    .collect::<Vec<_>>()[1],
            );
        }
        Ok(PodSummary {
            name: name.to_string(),
            age,
            phase,
            version,
            running,
            containers,
            restarts,
        })
    }
}

/// A summary of a ReplicaSet's status
#[derive(Debug)]
pub struct ReplicaSetSummary {
    pub hash: String,
    pub version: String,
    pub replicas: i32,
    pub ready: i32,
}

impl TryFrom<ReplicaSet> for ReplicaSetSummary {
    type Error = crate::Error;

    /// Helper to convert the openapi ReplicaSet to the useful info
    fn try_from(rs: ReplicaSet) -> Result<ReplicaSetSummary> {
        if let Some(status) = rs.status.clone() {
            let replicas = status.replicas;
            let ready = status.ready_replicas.unwrap_or(0);
            let mut ver = None;
            if let Some(spec) = &rs.spec {
                if let Some(tpl) = &spec.template {
                    if let Some(podspec) = &tpl.spec {
                        let containers = &podspec.containers;
                        let image = containers[0].image.clone().unwrap_or(":".to_string());
                        let tag = image.split(':').collect::<Vec<_>>()[1];
                        ver = Some(short_ver(tag));
                    }
                }
            };
            let version = ver.unwrap_or_else(|| "unknown version".to_string());
            let hash = match Meta::meta(&rs)
                .labels
                .clone()
                .unwrap_or_default()
                .get("pod-template-hash")
            {
                Some(h) => h.to_owned(),
                None => bail!("Need pod-template-hash from replicaset for {}", Meta::name(&rs)),
            };
            Ok(ReplicaSetSummary {
                hash,
                version,
                replicas,
                ready,
            })
        } else {
            bail!("Missing replicaset status object")
        }
    }
}

/// Debug why a workload is in the state it is in
pub async fn debug(mf: &Manifest, kube: &ShipKube) -> Result<()> {
    match mf.workload {
        PrimaryWorkload::Deployment => debug_deployment(kube).await,
        PrimaryWorkload::Statefulset => debug_statefulset(kube).await,
    }
}

/// Debug a deployment
///
/// Finds active replicasets (with pods in them)
/// Debugs the pods in each replicaset
/// Tails the logs from each broken pod
async fn debug_deployment(kube: &ShipKube) -> Result<()> {
    for rs in kube.get_rs().await? {
        if let Ok(r) = ReplicaSetSummary::try_from(rs) {
            // NB: ^ ignore replicasets that didn't parse
            if r.replicas == 0 {
                continue; // also ignore empty ones..
            }
            info!("{} Pod ReplicaSet {} running {}", r.replicas, r.hash, r.version);
            let pods = kube.get_pods_by_template_hash(&r.hash).await?;
            info!("Replicaset contains:");
            debug_pods(pods, kube).await?;
        }
    }
    Ok(())
}

async fn debug_statefulset(kube: &ShipKube) -> Result<()> {
    // For now, just list the pods as if there were no replicaset to worry about
    let pods = kube.get_pods().await?;
    info!("Statefulset contains:");
    debug_pods(pods, kube).await?;
    Ok(())
}

async fn debug_pods(pods: ObjectList<Pod>, kube: &ShipKube) -> Result<()> {
    for pod in pods {
        let podstate = PodSummary::try_from(pod)?;
        println!("{:?}", podstate);
        if podstate.running != podstate.containers as i32 {
            info!(
                "Fetching logs from non-ready main container in pod: {}",
                podstate.name
            );
            match kube.get_pod_logs(&podstate.name).await {
                Ok(logs) => {
                    warn!("Last 30 log lines:");
                    println!("{}", logs)
                }
                Err(e) => warn!("Failed to get logs from {}: {}", podstate.name, e),
            }
        }
    }
    Ok(())
}

/// A summary of a Deployment's status
#[derive(Debug)]
pub struct DeploySummary {
    pub replicas: i32,
    pub unavailable: i32,
    pub ready: i32,
    pub new_replicas_available: bool,
    pub message: Option<String>,
}

impl TryFrom<Deployment> for DeploySummary {
    type Error = crate::Error;

    /// Helper to convert the openapi Deployment to the useful info
    fn try_from(d: Deployment) -> Result<DeploySummary> {
        if let Some(status) = d.status {
            let ready = status.ready_replicas.unwrap_or(0);
            let unavailable = status.unavailable_replicas.unwrap_or(0);
            let replicas = status.replicas.unwrap_or(0);

            // Sometimes kube tells us in an obscure way that the rollout is done:
            let mut message = None;
            let mut new_replicas_available = false;
            if let Some(conds) = status.conditions {
                // This is a shortcut that works in kubernetes 1.15
                // We can't take advantage of this condition yet.
                if let Some(pcond) = conds.iter().find(|c| c.type_ == "Progressing") {
                    if let Some(reason) = &pcond.reason {
                        message = pcond.message.clone();
                        if reason == "NewReplicaSetAvailable" {
                            new_replicas_available = true;
                        }
                    }
                }
            }
            Ok(DeploySummary {
                replicas,
                unavailable,
                ready,
                message,
                new_replicas_available,
            })
        } else {
            bail!("Missing deployment status object")
        }
    }
}

/// A summary of a Statefulset's status
pub struct StatefulSummary {
    pub replicas: i32,
    pub ready: i32,
    pub current_revision: Option<String>,
    pub current_replicas: i32,
    pub update_revision: Option<String>,
    pub updated_replicas: i32,
}

impl TryFrom<StatefulSet> for StatefulSummary {
    type Error = crate::Error;

    /// Helper to convert the openapi Statefulset to the useful info
    fn try_from(d: StatefulSet) -> Result<StatefulSummary> {
        if let Some(status) = d.status {
            let ready = status.ready_replicas.unwrap_or(0);
            let replicas = status.replicas;
            let current_revision = status.current_revision;
            let current_replicas = status.current_replicas.unwrap_or(0);
            let update_revision = status.update_revision;
            let updated_replicas = status.updated_replicas.unwrap_or(0);

            // NB: No good message in statefulset conditions at 1.13
            Ok(StatefulSummary {
                replicas,
                ready,
                current_revision,
                current_replicas,
                update_revision,
                updated_replicas,
            })
        } else {
            bail!("Missing statefulset status object")
        }
    }
}

#[derive(Debug)]
struct RolloutResult {
    progress: u32,
    expected: u32,
    message: Option<String>,
    ok: bool,
}

/// Check if a rollout has completed
async fn rollout_status(mf: &Manifest, kube: &ShipKube, hash: &Option<String>) -> Result<RolloutResult> {
    match mf.workload {
        PrimaryWorkload::Deployment => {
            // Get root data from Deployment status
            let deploy = kube.get_deploy().await?;
            let d = DeploySummary::try_from(deploy)?;
            debug!("{}: {:?}", mf.name, d);
            // Wait for at least the minimum number...

            let mut acurate_progress = None; // accurate progress number
            let mut minimum = mf.min_replicas(); // minimum replicas we wait for
            if let Some(tpl_hash) = hash {
                // Infer from pinned ReplicaSet status (that was latest during apply)
                if let Some(rs) = kube.get_rs_by_template_hash(&tpl_hash).await? {
                    let r = ReplicaSetSummary::try_from(rs)?;
                    debug!("{}: {:?}", mf.name, r);
                    acurate_progress = Some(r.ready);
                    // rs might have scaled it up during rollout
                    minimum = std::cmp::max(minimum, r.replicas.try_into().unwrap_or(0));
                }
            }

            // Decide whether to stop polling - did the upgrade pass?
            let ok = if let Some(acc) = acurate_progress {
                // Replicaset is scaled to our minimum, and all ready
                // NB: k8s >= 1.15 we could use `d.new_replicas_available`
                // as a better required check
                acc == (minimum as i32)
            // NB: This last && enforces the progress downscaling at the end of fn
            } else {
                // FALLBACK (never seems to really happen): count from deployment only
                // Need to at least have as many ready as expected
                // ...it needs to have been scaled to the correct minimum
                // ...and, either we have the explicit progress done, or all unavailables are gone
                // The last condition (which increases waiting time) is necessary because:
                // deployment summary aggregates up the total number of ready pods
                // so we won't really know we're done or not unless we got the go-ahead
                // (i.e. d.new_replicas_available in k8s >= 1.15),
                // or all the unavailable pods have been killed (indicating total completeness)
                d.ready == d.replicas
                    && d.ready >= minimum as i32
                    && (d.new_replicas_available || d.unavailable <= 0)
            };

            //  What to tell our progress bar:
            let progress: i32 = match acurate_progress {
                // 99% case: the number from our accurately matched replicaset:
                Some(p) => p,

                // Otherwise estimate based on deployment.status data
                // Slightly weird data because of replicasets worth of data is here..
                // There might be more than one deployment in progress, all of which surge..
                None => std::cmp::max(0, d.ready - d.unavailable),
            };
            Ok(RolloutResult {
                progress: progress.try_into().expect("progress >= 0"),
                expected: minimum,
                message: d.message,
                ok,
            })
        }
        PrimaryWorkload::Statefulset => {
            let ss = kube.get_statefulset().await?;
            let s = StatefulSummary::try_from(ss)?;
            let minimum = mf.min_replicas();

            let ok = s.updated_replicas >= minimum as i32
                && s.updated_replicas == s.ready
                && s.update_revision == *hash;
            let message = if ok {
                None
            } else {
                Some("Statefulset update in progress".to_string())
            };

            // NB: Progress is slightly optimistic because updated_replicas increment
            // as soon as the old replica is replaced, not when it's ready.
            // (we can't use ready_replicas because that counts the sum of old + new)
            // But this is OK. If it gets to 3/3 then at least 2 rolled out successfully,
            // and the third was started. The only other way of getting around that
            // would be tracking the pods with the new hash directly..

            // Note that while progressbar is optimistic, it's not marked as ok (done)
            // until the new revision is changed over (when sts controller thinks it's done)
            // So this is a progressbar only inconsistency.
            Ok(RolloutResult {
                progress: std::cmp::max(0, s.updated_replicas)
                    .try_into()
                    .expect("sts.updated_replicas >= 0"),
                expected: minimum,
                message: message,
                ok,
            })
        }
    }
}

/// Track the rollout of the main workload
pub async fn workload_rollout(mf: &Manifest, kube: &ShipKube) -> Result<bool> {
    use futures_timer::Delay;
    use indicatif::{ProgressBar, ProgressStyle};
    let minimum = mf.min_replicas();
    let waittime = mf.estimate_wait_time();
    let one_sec = std::time::Duration::from_millis(1000);

    match rollout_status(mf, kube, &None).await {
        Ok(rr) => {
            if rr.ok {
                return Ok(true);
            } else {
                debug!("Ignoring rollout failure right after upgrade")
            }
        }
        Err(e) => warn!("Ignoring rollout failure right after upgrade: {}", e),
    };

    Delay::new(one_sec).await;
    // TODO: Don't count until image has been pulled + handle unscheduleble - #96

    info!(
        "Waiting {}s for {:?} {} to rollout (not ready yet)",
        waittime, mf.workload, mf.name
    );
    let mut hash = None;
    match mf.workload {
        PrimaryWorkload::Deployment => {
            // Attempt to find an owning RS hash to track
            if let Some(rs) = kube.get_rs_from_deploy().await? {
                if let Some(meta) = rs.metadata {
                    if let Some(labels) = meta.labels {
                        if let Some(h) = labels.get("pod-template-hash") {
                            debug!("Tracking replicaset {} for {}", h, mf.name);
                            hash = Some(h.clone());
                        }
                    }
                }
            }
        }
        PrimaryWorkload::Statefulset => {
            // Attempt to find an owning revesion hash to track
            let sts = kube.get_statefulset().await?;
            let summary = StatefulSummary::try_from(sts)?;
            if let Some(ur) = summary.update_revision {
                debug!("Tracking statefulset {:?} for {}", ur, mf.name);
                hash = Some(ur);
            }
        }
    }

    // TODO: create progress bar above this fn so we can use MultiProgressBar in cluster.rs
    let pb = ProgressBar::new(minimum.into());
    pb.set_style(
        ProgressStyle::default_bar()
            .template("> {bar:40.green/black} {prefix} {pos}/{len} ({elapsed}) {msg}"),
    );
    pb.set_draw_delta(1);
    if let Some(h) = &hash {
        match mf.workload {
            PrimaryWorkload::Deployment => {
                pb.set_prefix(&format!("{}-{}", mf.name, h));
            }
            PrimaryWorkload::Statefulset => {
                pb.set_prefix(h); // statefulset hash already prefixes name
            }
        }
    } else {
        pb.set_prefix(&mf.name);
    }

    for i in 1..20 {
        trace!("poll iteration {}", i);
        let mut waited = 0;
        // sleep until 1/20th of estimated upgrade time and poll for status
        while waited < waittime / 20 {
            waited += 1;
            trace!("sleep 1s (waited {})", waited);
            Delay::new(one_sec).await;
        }
        let rr = rollout_status(mf, kube, &hash).await?;
        debug!("RR: {:?}", rr);
        if let Some(msg) = rr.message {
            pb.set_message(&msg);
        }
        pb.set_length(rr.expected.into()); // sometimes a replicaset resizes
        pb.set_position(rr.progress.into());
        if rr.ok {
            pb.finish_at_current_pos();
            return Ok(true);
        }
    }
    Ok(false) // timeout
}
