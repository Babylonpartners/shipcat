use super::{Config, Region, Webhook};
use crate::{apply::UpgradeInfo, audit, slack, Result};

/// The different states an upgrade can be in
#[derive(Serialize, PartialEq, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpgradeState {
    /// Before action
    Pending,
    /// Action was cancelled before start
    Cancelled,
    // Action has started
    Started,
    /// No errors
    Completed,
    /// Errors
    Failed,
}

pub fn ensure_requirements(reg: &Region) -> Result<()> {
    for wh in &reg.webhooks {
        wh.get_configuration()?;
    }
    Ok(())
}

/// Throw events to configured webhooks - warning on delivery errors
///
/// Http errors SHOULD NOT be propagated from here
pub async fn reconcile_event(us: UpgradeState, reg: &Region) {
    for wh in &reg.webhooks {
        if let Ok(whc) = wh.get_configuration() {
            let res = match wh {
                Webhook::Audit(h) => audit::reconciliation(&us, &reg.name, &h, whc).await,
            };
            if let Err(e) = res {
                warn!("Failed to notify about reconciliation event: {}", e)
            }
        }
    }
}

/// Throw events to configured webhooks
pub async fn apply_event(us: UpgradeState, info: &UpgradeInfo, reg: &Region, conf: &Config) {
    debug!("Apply event: {:?}", info);
    // Webhooks defined in shipcat.conf for the region:
    for wh in &reg.webhooks {
        if let Ok(whc) = wh.get_configuration() {
            let res = match wh {
                Webhook::Audit(h) => {
                    match us {
                        UpgradeState::Started | UpgradeState::Completed | UpgradeState::Failed => {
                            audit::apply(&us, &info, &h, whc).await
                        }
                        _ => Ok(()), // audit only sends Started / Failed / Completed
                    }
                }
            };
            if let Err(e) = res {
                warn!("Failed to notify about apply event: {}", e)
            }
        }
    }
    // slack notifications:
    let (color, text) = match us {
        UpgradeState::Completed => ("good", format!("applied `{}` in `{}`", info.name, info.region)),
        UpgradeState::Failed => (
            "danger",
            format!("failed to apply `{}` in `{}`", info.name, info.region),
        ),
        _ => (
            "good",
            format!(
                "action state: {}",
                serde_json::to_string(&us).unwrap_or("unknown".into())
            ),
        ),
    };
    match us {
        UpgradeState::Completed | UpgradeState::Failed => {
            let _ = slack::send(
                slack::Message {
                    text,
                    code: info.diff.clone(),
                    color: Some(String::from(color)),
                    version: Some(info.version.clone()),
                    mode: info.slackMode.clone(),
                    metadata: info.metadata.clone(),
                },
                &conf.owners,
            )
            .await;
        }
        _ => {}
    }
}

/// Throw events to configured webhooks
///
/// This is the new version for shipcat apply module
pub async fn delete_event(us: &UpgradeState, info: &UpgradeInfo, reg: &Region, conf: &Config) {
    // Webhooks defined in shipcat.conf for the region:
    debug!("Delete event: {:?}", info);
    for wh in &reg.webhooks {
        if let Ok(whc) = wh.get_configuration() {
            let res = match wh {
                Webhook::Audit(h) => audit::deletion(&us, &info, &h, whc).await,
            };
            if let Err(e) = res {
                warn!("Failed to notify about delete event: {}", e)
            }
        }
    }
    // slack notifies when we start the deletion only
    #[allow(clippy::single_match)] // no PartialEq for UpgradeState
    match us {
        UpgradeState::Started => {
            let color = "warning";
            let text = format!("deleting `{}` in `{}`", info.name, reg.name);
            let _ = slack::send(
                slack::Message {
                    text,
                    code: info.diff.clone(),
                    color: Some(String::from(color)),
                    version: Some(info.version.clone()),
                    mode: info.slackMode.clone(),
                    metadata: info.metadata.clone(),
                },
                &conf.owners,
            )
            .await;
        }
        _ => {}
    };
}
