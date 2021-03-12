use super::Result;

// Untagged enum to get around the weird validation
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum AvailabilityPolicy {
    Percentage(String),
    Unsigned(u32),
}
// Kube has a weird hybrid type for this intstr.IntOrString: IntVal | StrVal
// if it's a string, then '[0-9]+%!' has to parse
impl AvailabilityPolicy {
    fn verify(&self, name: &str, maxNumber: u32) -> Result<()> {
        match self {
            AvailabilityPolicy::Unsigned(n) => {
                if *n > maxNumber {
                    bail!("Cannot have {} set higher than replicaCount {}", name, maxNumber);
                }
            }
            AvailabilityPolicy::Percentage(s) => {
                if !s.ends_with('%') {
                    bail!("{} must end with a '%' sign", name);
                }
                let digits = s.chars().take_while(|ch| *ch != '%').collect::<String>();
                let res: u32 = digits.parse()?;
                if res > 100 {
                    bail!("Percentage value for {} cannot exceed 100", name);
                }
            }
        }
        // TODO: ensure both not zero (illegal - currently caught by apiserver)
        Ok(())
    }

    /// Figure out how many the availability policy refers to
    ///
    /// This multiplies the policy with num replicas and rounds up (for maxSurge)
    fn to_replicas_ceil(&self, replicas: u32) -> u32 {
        match self {
            AvailabilityPolicy::Percentage(percstr) => {
                let digits = percstr.chars().take_while(|ch| *ch != '%').collect::<String>();
                let surgeperc: u32 = digits.parse().unwrap(); // safe due to verify ^
                ((f64::from(replicas) * f64::from(surgeperc)) / 100.0).ceil() as u32
            }
            AvailabilityPolicy::Unsigned(u) => *u,
        }
    }

    /// Figure out how many the availability policy refers to
    ///
    /// This multiplies the policy with num replicas and rounds down (for maxUnavailable)
    fn to_replicas_floor(&self, replicas: u32) -> u32 {
        match self {
            AvailabilityPolicy::Percentage(percstr) => {
                let digits = percstr.chars().take_while(|ch| *ch != '%').collect::<String>();
                let surgeperc: u32 = digits.parse().unwrap(); // safe due to verify ^
                ((f64::from(replicas) * f64::from(surgeperc)) / 100.0).floor() as u32
            }
            AvailabilityPolicy::Unsigned(u) => *u,
        }
    }
}

/// Configuration parameters for Deployment.spec.strategy.rollingUpdate
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RollingUpdate {
    /// How many replicas or percentage of replicas that can be down during rolling-update
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxUnavailable: Option<AvailabilityPolicy>,
    /// Maximum number of pods that can be created over replicaCount
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxSurge: Option<AvailabilityPolicy>,
}

/// Implement Default that matches kubernetes
///
/// Both values defined in kube docs for deployment under .spec.strategy
/// https://kubernetes.io/docs/concepts/workloads/controllers/deployment/#writing-a-deployment-spec
impl Default for RollingUpdate {
    fn default() -> Self {
        RollingUpdate {
            maxUnavailable: Some(AvailabilityPolicy::Percentage(25.to_string())),
            maxSurge: Some(AvailabilityPolicy::Percentage(25.to_string())),
        }
    }
}

impl RollingUpdate {
    pub fn verify(&self, replicas: u32) -> Result<()> {
        if self.maxUnavailable.is_none() && self.maxSurge.is_none() {
            bail!("Need to set one of maxUnavailable or maxSurge in rollingUpdate");
        }
        if let Some(ref ma) = &self.maxUnavailable {
            ma.verify("maxUnavailable", replicas)?;
        }
        if let Some(ref mu) = &self.maxSurge {
            mu.verify("maxSurge", replicas)?;
        }
        Ok(())
    }
}

impl RollingUpdate {
    /// Estimate how many cycles is needed to roll out a new version
    ///
    /// This is a bit arcane extrapolates from [rolling update documentation](https://kubernetes.io/docs/concepts/workloads/controllers/deployment/#max-unavailable)
    /// It needs to keep into account both values.
    pub fn rollout_iterations(&self, replicas: u32) -> u32 {
        let surge = if let Some(surge) = self.maxSurge.clone() {
            // surge is max number/percentage
            surge.to_replicas_ceil(replicas)
        } else {
            // default surge percentage is 25
            (f64::from(replicas * 25) / 100.0).ceil() as u32
        };
        let unavail = if let Some(unav) = self.maxUnavailable.clone() {
            // maxUnavailable is max number/percentage
            unav.to_replicas_floor(replicas)
        } else {
            (f64::from(replicas * 25) / 100.0).floor() as u32
        };
        // Work out how many iterations is needed assuming consistent rollout time
        // Often, this is not true, but it provides a good indication
        let mut newrs = 0;
        let mut oldrs = replicas; // keep track of for ease of following logic
        let mut iters = 0;
        trace!(
            "rollout iterations for {} replicas, surge={},unav={}",
            replicas,
            surge,
            unavail
        );
        while newrs < replicas {
            // kill from oldrs the difference in total if we are surging
            oldrs -= oldrs + newrs - replicas; // noop if surge == 0
                                               // terminate pods so we have at least maxUnavailable
            let total = newrs + oldrs;

            let unavail_safe = if total <= unavail { 0 } else { unavail };
            trace!(
                "oldrs{}, total is {}, unavail_safe: {}",
                oldrs,
                total,
                unavail_safe
            );
            oldrs -= std::cmp::min(oldrs, unavail_safe); // never integer overflow
                                                         // add new pods to cover and allow surging a little
            newrs += unavail_safe;
            newrs += surge;
            // after this iteration, assume we have rolled out newrs replicas
            // and we hve ~_oldrs remaining (ignoring <0 case)
            iters += 1;
            trace!("rollout iter {}: old={}, new={}", iters, oldrs, newrs);
        }
        trace!("rollout iters={}", iters);
        iters
    }

    pub fn rollout_iterations_default(replicas: u32) -> u32 {
        // default surge percentage is 25
        ((f64::from(replicas) * 25.0) / 100.0).ceil() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{AvailabilityPolicy, RollingUpdate};

    #[test]
    fn rollout_iteration_no_overflow() {
        // ensure no interger failures above..
        for i in 0..100 {
            //println!("overflow check for {}", i);
            assert!(RollingUpdate::default().rollout_iterations(i) < 5);
        }
    }

    #[test]
    fn rollout_iteration_check() {
        // examples cross referenced with kube
        // (kube rollout cycles in parens after each assert)
        let ru = RollingUpdate::default();
        assert_eq!(ru.rollout_iterations(1), 1); // 0 dn 1 up (then 1 down ungated)
        assert_eq!(ru.rollout_iterations(2), 2); // 0 dn 1 up, 1 dn 2 up (then 1 down ungated)
        assert_eq!(ru.rollout_iterations(4), 2); // 1 dn 2 up, 2 dn 2 up
        assert_eq!(ru.rollout_iterations(8), 2); // 2 dn 4 up, 4 dn 4 up (then 2 down ungated)

        // an example that will surge quickly:
        let rusurge = RollingUpdate {
            maxUnavailable: Some(AvailabilityPolicy::Percentage("25%".to_string())),
            maxSurge: Some(AvailabilityPolicy::Percentage("50%".to_string())),
        };
        assert_eq!(rusurge.rollout_iterations(8), 2); // 2 dn 6  up, 6  dn 2 up
        assert_eq!(rusurge.rollout_iterations(16), 2); // 4 dn 12 up, 12 dn 4 up

        // an example that kill almost everything immediately
        let rusurge = RollingUpdate {
            maxUnavailable: Some(AvailabilityPolicy::Percentage("75%".to_string())),
            maxSurge: Some(AvailabilityPolicy::Percentage("25%".to_string())),
        };
        assert_eq!(rusurge.rollout_iterations(8), 1); // 6 dn 8 up (then 2 down ungated)

        // an example with no surge
        let rusurge = RollingUpdate {
            maxUnavailable: Some(AvailabilityPolicy::Percentage("25%".to_string())),
            maxSurge: Some(AvailabilityPolicy::Percentage("0%".to_string())),
        };
        assert_eq!(rusurge.rollout_iterations(8), 4); // 2 dn 2 up (x4)
    }
}
