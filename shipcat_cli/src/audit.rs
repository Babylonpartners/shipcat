use std::collections::BTreeMap;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use url::Url;
use uuid::Uuid;

use super::{AuditWebhook, ErrorKind, Result, ResultExt};
use crate::{apply::UpgradeInfo, webhooks::UpgradeState};

// Webhook Configuration Map
type WHC = BTreeMap<String, String>;

// ----------------------------------------------------------------------------------
// Audit Event definitions and sending
// ----------------------------------------------------------------------------------

/// Payload that gets sent via audit webhook
///
/// Generic over the payload type, defined below for different domain_types
#[derive(Serialize, Clone)]
struct AuditEvent<T: Serialize + Clone> {
    /// Payload type
    #[serde(rename = "type")]
    domain_type: String,
    /// RFC 3339
    timestamp: String,
    status: UpgradeState,
    /// Eg a jenkins job id
    context_id: String,
    /// Eg a jenkins job url
    #[serde(skip_serializing_if = "Option::is_none")]
    context_link: Option<Url>,

    /// represents a single kubectl apply, kubectl delete, or a reconciliation
    payload: T,
}

impl<T> AuditEvent<T>
where
    T: Serialize + Clone,
{
    fn new(at: AuditType, whc: &WHC, status: &UpgradeState, payload: T) -> Self {
        AuditEvent {
            domain_type: at.to_string(),
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            status: status.clone(),
            context_id: whc["SHIPCAT_AUDIT_CONTEXT_ID"].clone(),
            context_link: whc
                .get("SHIPCAT_AUDIT_CONTEXT_LINK")
                .and_then(|l| Url::parse(&l).ok()),
            payload,
        }
    }

    async fn send(&self, audcfg: &AuditWebhook) -> Result<()> {
        let endpoint = &audcfg.url;
        debug!(
            "event status: {}, url: {:?}",
            serde_json::to_string(&self.status)?,
            endpoint
        );

        reqwest::Client::new()
            .post(endpoint.clone())
            .bearer_auth(audcfg.token.clone())
            .json(&self)
            .send()
            .await
            .chain_err(|| ErrorKind::Url(endpoint.clone()))?;
        Ok(())
    }
}

// ----------------------------------------------------------------------------------
// audit types and their payloads
// ----------------------------------------------------------------------------------

#[derive(Debug)]
enum AuditType {
    Deployment,
    Reconciliation,
    Deletion,
}
impl ToString for AuditType {
    fn to_string(&self) -> String {
        format!("{:?}", self).to_lowercase()
    }
}

// Payload for Deployment (apply) events
#[derive(Serialize, Clone)]
struct DeploymentPayload {
    id: String,
    region: String,
    service: String,
    version: String,
    manifests_revision: String,
}
impl DeploymentPayload {
    fn new(whc: &WHC, info: &UpgradeInfo) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            region: info.region.clone(),
            service: info.name.clone(),
            version: info.version.clone(),
            manifests_revision: whc["SHIPCAT_AUDIT_REVISION"].clone(),
        }
    }
}

// Payload for Reconciliation (crd reconcile) events
#[derive(Serialize, Clone)]
struct ReconciliationPayload {
    id: String,
    region: String,
    manifests_revision: String,
}
impl ReconciliationPayload {
    fn new(whc: &WHC, r: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            region: r.into(),
            manifests_revision: whc["SHIPCAT_AUDIT_REVISION"].clone(),
        }
    }
}

// Payload for Deletion (crd reconcile) events
#[derive(Serialize, Clone)]
struct DeletionPayload {
    id: String,
    region: String,
    service: String,
    manifests_revision: String,
}
impl DeletionPayload {
    fn new(whc: &WHC, info: &UpgradeInfo) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            manifests_revision: whc["SHIPCAT_AUDIT_REVISION"].clone(),
            region: info.region.clone(),
            service: info.name.clone(),
        }
    }
}

// ----------------------------------------------------------------------------------
// public interface of things to audit
// ----------------------------------------------------------------------------------

/// Apply audit sent by shipcat::aplpy
pub async fn apply(us: &UpgradeState, u: &UpgradeInfo, audcfg: &AuditWebhook, whc: WHC) -> Result<()> {
    let pl = DeploymentPayload::new(&whc, &u);
    AuditEvent::new(AuditType::Deployment, &whc, &us, pl)
        .send(&audcfg)
        .await
}

/// Apply audit sent by shipcat::cluster
pub async fn reconciliation(us: &UpgradeState, region: &str, audcfg: &AuditWebhook, whc: WHC) -> Result<()> {
    let pl = ReconciliationPayload::new(&whc, region);
    AuditEvent::new(AuditType::Reconciliation, &whc, &us, pl)
        .send(&audcfg)
        .await
}

/// Delete audit sent by shipcat::cluster
pub async fn deletion(us: &UpgradeState, ui: &UpgradeInfo, audcfg: &AuditWebhook, whc: WHC) -> Result<()> {
    let pl = DeletionPayload::new(&whc, &ui);
    AuditEvent::new(AuditType::Deletion, &whc, &us, pl)
        .send(&audcfg)
        .await
}

// ----------------------------------------------------------------------------------
// tests
// ----------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use url::Url;

    use crate::{apply::UpgradeInfo, audit, AuditWebhook, Manifest, Result, UpgradeState};

    #[tokio::test]
    async fn audit_does_audit_deployment() -> Result<()> {
        let mut whc: BTreeMap<String, String> = BTreeMap::default();
        whc.insert("SHIPCAT_AUDIT_CONTEXT_ID".into(), "egcontextid".into());
        whc.insert("SHIPCAT_AUDIT_CONTEXT_LINK".into(), "http://eg.server/".into());
        whc.insert("SHIPCAT_AUDIT_REVISION".into(), "egrevision".into());

        let audcfg = AuditWebhook {
            url: Url::parse(&format!("{}/audit", mockito::server_url()))?,
            token: "1234auth".into(),
        };
        let mf = Manifest::test("fake-svc"); // for dev-uk 1.0.0

        let us = UpgradeState::Completed;
        let ud = UpgradeInfo::new(&mf);

        let mocked = mockito::mock("POST", "/audit")
            .match_header("content-type", "application/json")
            .match_header("Authorization", "Bearer 1234auth")
            .match_body(mockito::Matcher::PartialJson(serde_json::json!({
                "status": "COMPLETED",
                "context_id": "egcontextid",
                "context_link": "http://eg.server/",
                "type": "deployment",
                "payload": {
                    "manifests_revision": "egrevision",
                    // NB: these 3 strings rely on mf props, esp. Manifest::test
                    "service": "fake-svc",
                    "region": "dev-uk",
                    "version": "1.0.0"
                }
            })))
            .expect(1)
            .create();
        audit::apply(&us, &ud, &audcfg, whc).await?;
        mocked.assert();
        Ok(())
    }

    #[test]
    fn audit_reconciliation_has_type() {
        let mut whc: BTreeMap<String, String> = BTreeMap::default();
        whc.insert("SHIPCAT_AUDIT_CONTEXT_ID".into(), "egcontextid".into());
        whc.insert("SHIPCAT_AUDIT_CONTEXT_LINK".into(), "http://eg.server/".into());
        whc.insert("SHIPCAT_AUDIT_REVISION".into(), "egrevision".into());

        let arp = audit::ReconciliationPayload::new(&whc, "region_name");
        let ae = audit::AuditEvent::new(
            audit::AuditType::Reconciliation,
            &whc,
            &UpgradeState::Completed,
            arp,
        );
        assert_eq!(ae.domain_type, "reconciliation");
    }
}
