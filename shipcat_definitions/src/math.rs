use super::{
    structs::{rollingupdate::RollingUpdate, ResourceRequirements},
    Manifest, Result,
};

/// Total resource usage for a Manifest
///
/// Accounting for workers, replicas, sidecars, and autoscaling policies for these.
#[derive(Serialize, Default)]
pub struct ResourceTotals {
    /// Sum of basic resource structs (ignoring autoscaling limits)
    pub base: ResourceRequirements<f64>,
    /// Autoscaling Ceilings on top of required
    pub extra: ResourceRequirements<f64>,
}

impl ResourceTotals {
    /// Round all numbers to gigs and full cores (for all teams)
    pub fn normalise(mut self) -> Self {
        self.base.round();
        self.extra.round();
        self
    }

    /// Compute daily cost lower + upper bounds based on instance cost
    ///
    /// Assumes the resource totals have been normalise first!
    pub fn daily_cost(&self) -> (f64, f64) {
        // instance cost is hourly cost for a resource.
        // E.g. m5.2xlarge => $0.384 per Hour, but has 31GB ram, 8vCPU
        let icost = 0.384 * 24.0;
        let node_mem = 31.0;
        let node_cpu = 8.0;

        let memory_cost = (
            (self.base.requests.memory * icost / node_mem).round(),
            ((self.base.requests.memory + self.extra.requests.memory) * icost / node_mem).round(),
        );
        let cpu_cost = (
            (self.base.requests.cpu * icost / node_cpu).round(),
            ((self.base.requests.cpu + self.extra.requests.cpu * icost) * icost / node_cpu).round(),
        );
        // quick extimate at what would be more expensive
        if cpu_cost.1 > memory_cost.1 {
            cpu_cost
        } else {
            memory_cost
        }
    }
}

/// Calculations done based on values in manifests
///
/// These generally assume that `verify` has passed on all manifests.
impl Manifest {
    /// Compute minimum replicas
    ///
    /// Used to `estimate_rollout_iterations` for a rollout.
    pub fn min_replicas(&self) -> u32 {
        if let Some(ref hpa) = self.autoScaling {
            hpa.minReplicas
        } else {
            self.replicaCount.unwrap() // verify ensures we have one of these
        }
    }

    /// Estimate how many iterations needed in a kube rolling upgrade
    ///
    /// Used to `estimate_wait_time` for a rollout.
    pub fn estimate_rollout_iterations(&self) -> u32 {
        let rcount = self.min_replicas();
        if let Some(ru) = self.rollingUpdate.clone() {
            ru.rollout_iterations(rcount)
        } else {
            RollingUpdate::default().rollout_iterations(rcount)
        }
    }

    /// Estimate how long to wait for a kube rolling upgrade
    ///
    /// Was used by helm, now used by the internal upgrade wait time.
    pub fn estimate_wait_time(&self) -> u32 {
        // TODO: handle install case elsewhere..
        if let Some(size) = self.imageSize {
            // 512 default => extra 90s wait, then 90s per half gig...
            // TODO: smoothen..
            let pulltimeestimate = std::cmp::max(60, ((f64::from(size) * 90.0) / 512.0) as u32);
            let rollout_iterations = self.estimate_rollout_iterations();
            // println!("estimating wait for {} cycle rollout: size={} (est={})", rollout_iterations, size, pulltimeestimate);

            // how long each iteration needs to wait due to readinessProbe params.
            let delayTimeSecs = if let Some(ref hc) = self.health {
                hc.wait
            } else if let Some(ref rp) = self.readinessProbe {
                rp.initialDelaySeconds
            } else {
                30 // guess value in weird case where no health / readiessProbe
            };
            // give it some leeway
            let delayTime = (f64::from(delayTimeSecs) * 1.5).ceil() as u32;
            // leeway scales linearly with wait because we assume accuracy goes down..

            // Final formula: (how long to wait to poll + how long to pull) * num cycles
            (delayTime + pulltimeestimate) * rollout_iterations
        } else {
            warn!("Missing imageSize in {}", self.name);
            300 // helm default --timeout value
        }
    }

    /// Compute the total resource usage of a service
    ///
    /// This relies on the `Mul` and `Add` implementations of `ResourceRequirements<f64>`,
    /// which allows us to do `+` and `*` on a normalised ResourceRequirements struct.
    pub fn compute_resource_totals(&self) -> Result<ResourceTotals> {
        let mut base: ResourceRequirements<f64> = ResourceRequirements::default();
        let mut extra: ResourceRequirements<f64> = ResourceRequirements::default(); // autoscaling limits
        let res = self.resources.clone().unwrap().normalised()?; // exists by verify
        if let Some(ref ascale) = self.autoScaling {
            base += res.clone() * ascale.minReplicas;
            extra += res * (ascale.maxReplicas - ascale.minReplicas);
        } else if let Some(rc) = self.replicaCount {
            // can trust the replicaCount here
            base += res * rc;
            for s in &self.sidecars {
                if let Some(ref scrsc) = s.resources {
                    // sidecar replicaCount == main deployment replicaCount
                    base += scrsc.normalised()? * rc;
                }
                // TODO: mandatory? sidecar resources when using sidecars?
            }
        } else {
            bail!("{} does not have replicaCount", self.name);
        }
        for w in &self.workers {
            if let Some(resources) = &w.container.resources {
                base += resources.normalised()? * w.replicaCount;
            }
            // TODO: account for autoscaling in workers when it's there

            // NB: workers get the same sidecars!
            for s in &self.sidecars {
                if let Some(ref scrsc) = s.resources {
                    // worker sidecar replicaCount == worker deployment replicaCount
                    base += scrsc.normalised()? * w.replicaCount;
                }
                // TODO: mandatory? sidecar resources when using sidecars?
            }
        }
        Ok(ResourceTotals { base, extra })
    }
}

#[cfg(test)]
mod tests {
    use super::Manifest;
    use crate::structs::HealthCheck;

    #[test]
    fn mf_wait_time_check() {
        // standard setup - 300s wait is helm default
        let mut mf = Manifest::default();
        mf.imageSize = Some(512);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 60,
            ..Default::default()
        });
        mf.replicaCount = Some(1);
        assert_eq!(mf.estimate_wait_time(), 180); // 60*1.5 + 90s
        mf.replicaCount = Some(2); // needs two cycles now
        assert_eq!(mf.estimate_wait_time(), 360); // (60*1.5 + 90s)*2

        // huge image, fast boot
        // causes some pretty high numbers atm - mostly there to catch variance
        // this factor can be scaled down in the future
        mf.imageSize = Some(4096);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 10,
            ..Default::default()
        });
        mf.replicaCount = Some(1);
        assert_eq!(mf.estimate_wait_time(), 735); // very high.. network not always reliable

        // medium images, sloooow boot
        mf.imageSize = Some(512);
        mf.health = Some(HealthCheck {
            uri: "/".into(),
            wait: 600,
            ..Default::default()
        });
        mf.replicaCount = Some(1);
        assert_eq!(mf.estimate_wait_time(), 990); // lots of leeway here just in case
    }
}
