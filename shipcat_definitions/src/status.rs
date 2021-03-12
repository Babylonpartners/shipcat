use super::Result;
use chrono::{SecondsFormat, Utc};

pub fn make_date() -> String {
    // Format == `1996-12-19T16:39:57-08:00`, but we hardcode Utc herein.
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Status object for shipcatmanifests crd
///
/// All fields optional, but we try to ensure all fields exist.
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManifestStatus {
    /// Detailed individual conditions, emitted as they happen during apply
    #[serde(default)]
    pub conditions: Conditions,
    /// A more easily readable summary of why the conditions are what they are
    #[serde(default)]
    pub summary: Option<ConditionSummary>,
    /* TODO: vault secret hash
     * MAYBE: kong status?
     * MAYBE: canary status? */
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Conditions {
    /// Generated
    ///
    /// If this .status is false, this might contain information about:
    /// - manifest failing to complete
    /// - temporary manifest files failing to write to disk
    /// - manifests failing to serialize
    /// - secrets failing to resolve
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<Condition>,

    /// Applied status
    ///
    /// If applied.status is false, this might contain information about:
    /// - invalid yaml when combining charts and values
    /// - configuration not passing admission controllers logic
    /// - network errors when applying
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<Condition>,

    /// Rollout of current shipcatmanifest succeeded
    ///
    /// If rollout.status is false, this might contain information about:
    /// - deployment(s) failing to roll out in time
    /// - network errors tracking the rollout
    /// Best effort information given in message, but this won't replace DeploymentConditions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolledout: Option<Condition>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConditionSummary {
    /// Date string (RFC3339) of when we generated the template successfully
    #[serde(default)]
    last_successful_generate: Option<String>,

    /// Date string (RFC3339) of when we last applied manifest configuration
    #[serde(default)]
    pub last_apply: Option<String>,

    /// Date string (RFC3339) of when an apply passed all checks
    #[serde(default)]
    last_successful_apply: Option<String>,

    /// Date string (RFC3339) of when a rollout wait completed
    #[serde(default)]
    last_rollout: Option<String>,

    /// Date string (RFC3339) of when a rollout wait completed and passed
    #[serde(default)]
    last_successful_rollout: Option<String>,

    // last action we performed
    #[serde(default)]
    last_action: Option<String>,

    /// reason for last failure (if any)
    #[serde(default)]
    last_failure_reason: Option<String>,

    /// Best effort reason for why an apply was triggered
    #[serde(default)]
    last_apply_reason: Option<String>,

    /// Last version that was successfully rolled out
    #[serde(default)]
    pub last_successful_rollout_version: Option<String>,
}

/// Condition
///
/// Stated out like a normal kubernetes conditions like PodCondition:
///
///  - lastProbeTime: null
///    lastTransitionTime: "2019-07-31T13:07:30Z"
///    message: 'containers with unready status: [product-config]'
///    reason: ContainersNotReady
///    status: "False"
///    type: ContainersReady
///
/// where we ignore lastProbeTime / lastHeartbeatTime because they are expensive,
/// and we add in an originator/source of the condition for parallel setups.
///
/// However, due to the lack of possibilities for patching statuses and general
/// difficulty dealing with the vector struct, we instead have multiple named variants.
///
/// See https://github.com/kubernetes/kubernetes/issues/7856#issuecomment-323196033
/// and https://github.com/clux/kube-rs/issues/43
/// For the reasoning.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Condition {
    /// Whether or not in a good state
    ///
    /// This must default to true when in a good state
    pub status: bool,
    /// Error reason type if not in a good state
    #[serde(default)]
    pub reason: Option<String>,
    /// One sentence error message if not in a good state
    #[serde(default)]
    pub message: Option<String>,

    /// When the condition was last written (RFC 3339 timestamp)
    #[serde(rename = "lastTransitionTime")]
    pub last_transition: String,

    /// Originator for this condition
    #[serde(default)]
    pub source: Option<Applier>,
}

impl Condition {
    pub fn ok(a: &Applier) -> Self {
        Condition {
            status: true,
            source: Some(a.clone()),
            last_transition: make_date(),
            reason: None,
            message: None,
        }
    }

    pub fn bad(a: &Applier, err: &str, msg: String) -> Self {
        Condition {
            status: false,
            source: Some(a.clone()),
            last_transition: make_date(),
            reason: Some(err.into()),
            message: Some(msg),
        }
    }

    pub fn format_last_transition(&self) -> Result<String> {
        use chrono::{DateTime, Duration};
        let old_ts = &self.last_transition;
        let last = old_ts.parse::<DateTime<Utc>>()?;
        let diff: Duration = Utc::now() - last;
        let days = diff.num_days();
        let hours = diff.num_hours();
        let mins = diff.num_minutes();
        let diff_fmt = if days >= 1 {
            let plural = if days > 1 { "s" } else { "" };
            format!("{} day{}", days, plural)
        } else if hours >= 1 {
            let plural = if hours > 1 { "s" } else { "" };
            format!("{} hour{}", hours, plural)
        } else {
            let plural = if mins > 1 { "s" } else { "" };
            format!("{} minute{}", mins, plural)
        };
        Ok(diff_fmt)
    }

    pub fn html_list_item(&self) -> Result<String> {
        let mut s = String::from("");
        match self.format_last_transition() {
            Ok(when) => s += &format!("{} ago", when),
            Err(e) => warn!("failed to parse timestamp from condition: {}", e),
        }
        if let Some(src) = &self.source {
            let via = if let Some(url) = &src.url {
                format!("<a href=\"{}\">{}</a>", url, src.name)
            } else {
                src.name.clone()
            };
            s += &format!(" via {}", via);
        }
        if self.status {
            s += " (Success)";
        } else if let (Some(r), Some(msg)) = (&self.reason, &self.message) {
            s += &format!(" ({}: {})", r, msg);
        } else {
            s += " (Failure)"; // no reason!?
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{Applier, Condition};
    use chrono::{prelude::*, Utc};
    #[test]
    #[ignore]
    fn check_conditions() {
        let applier = Applier {
            name: "clux".into(),
            url: None,
        };
        let mut cond = Condition::ok(&applier);
        cond.last_transition = Utc
            .ymd(1996, 12, 19)
            .and_hms(16, 39, 57)
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let encoded = serde_yaml::to_string(&cond).unwrap();
        println!("{}", encoded);
        assert!(encoded.contains("status: true"));
        assert!(encoded.contains("lastTransitionTime: \"1996-12-19T16:39:57+00:00\""));
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Applier {
    /// Human readable text describing what applied
    pub name: String,
    /// Link to logs or origin of the apply (if possible)
    #[serde(default)]
    pub url: Option<String>,
}

impl Applier {
    /// Infer originator of an apply
    pub fn infer() -> Applier {
        use std::env;
        if let (Ok(url), Ok(name), Ok(nr)) = (
            env::var("BUILD_URL"),
            env::var("JOB_NAME"),
            env::var("BUILD_NUMBER"),
        ) {
            // we are on jenkins
            Applier {
                name: format!("{}#{}", name, nr),
                url: Some(url),
            }
        } else if let (Ok(url), Ok(name), Ok(nr)) = (
            env::var("CIRCLE_BUILD_URL"),
            env::var("CIRCLE_JOB"),
            env::var("CIRCLE_BUILD_NUM"),
        ) {
            // we are on circle
            Applier {
                name: format!("{}#{}", name, nr),
                url: Some(url),
            }
        } else if let Ok(user) = env::var("USER") {
            Applier {
                name: user,
                url: None,
            }
        } else {
            warn!("Could not infer applier from this environment");
            // TODO: maybe lock down this..
            Applier {
                name: "unknown origin".into(),
                url: None,
            }
        }
    }
}
