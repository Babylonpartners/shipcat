use super::{ErrorKind, Manifest, Result};
use kube::{
    api::{Api, PostParams},
    client::APIClient,
    config::load_kube_config,
};
use serde::Serialize;
use tokio::process::Command;

use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};

struct AccessReviewRequest {
    namespace: String,
    verb: String,
    resource: String,
    subresource: Option<String>,
}

pub async fn kexec(args: Vec<String>) -> Result<()> {
    debug!("kubectl {}", args.join(" "));
    let s = Command::new("kubectl").args(&args).status().await?;
    if !s.success() {
        bail!("Subprocess failure from kubectl: {}", s.code().unwrap_or(1001))
    }
    Ok(())
}
async fn kout(args: Vec<String>) -> Result<(String, bool)> {
    debug!("kubectl {}", args.join(" "));
    let s = Command::new("kubectl").args(&args).output().await?;
    let out: String = String::from_utf8_lossy(&s.stdout).into();
    let err: String = String::from_utf8_lossy(&s.stderr).to_string().trim().into();
    if !err.is_empty() {
        warn!("kubectl {} stderr: {}", args.join(" "), err);
    }
    // kubectl keeps returning opening and closing apostrophes - strip them:
    if out.len() > 2 && out.starts_with('\'') {
        let res = out.split('\'').collect::<Vec<_>>()[1];
        return Ok((res.trim().into(), s.status.success()));
    }
    Ok((out, s.status.success()))
}
// fn get_kube_permissions(namespace: String) -> Result<Vec<ResourceRule>> {
// let config = load_kube_config().expect("config failed to load");
// let client = APIClient::new(config);
//
// let ssrr = RawApi::customResource("selfsubjectrulesreviews").group("authorization.k8s.io").version("v1");
//
// let pp = PostParams::default();
//
// let auth_req = Object{
// types: TypeMeta{
// kind: Some("SelfSubjectRulesReview".into()),
// apiVersion: Some("authorization.k8s.io/v1".into()),
// },
// metadata: ObjectMeta::default(),
// spec: SelfSubjectRulesReviewSpec{
// namespace: Some(namespace),
// },
// status: Some(SubjectRulesReviewStatus::default()),
// };
//
// let req = ssrr.create(&pp, serde_json::to_vec(&auth_req)?).expect("failed to create request");
// let o = client.request::<Object<SelfSubjectRulesReviewSpec, SubjectRulesReviewStatus>>(req).map_err(ErrorKind::KubeError)?;
//
// debug!("spec: {:?}", o.spec);
// debug!("status: {:?}", o.status);
// Ok(o.status.expect("expected rules").resource_rules)
// }
async fn kani(rr: AccessReviewRequest) -> Result<bool> {
    let config = load_kube_config().await.expect("config failed to load");
    let client = APIClient::new(config);

    let ssrr: Api<SelfSubjectAccessReview> = Api::all(client);
    let pp = PostParams::default();

    let mut auth_req = SelfSubjectAccessReview::default();
    auth_req.spec = SelfSubjectAccessReviewSpec {
        resource_attributes: Some(ResourceAttributes {
            namespace: Some(rr.namespace),
            verb: Some(rr.verb),
            resource: Some(rr.resource),
            subresource: rr.subresource,
            group: None,
            name: None,
            version: None,
        }),
        non_resource_attributes: None,
    };

    let o = ssrr.create(&pp, &auth_req).await.map_err(ErrorKind::KubeError)?;

    debug!("spec: {:?}", o.spec);
    let status = o.status.expect("expected status");

    if let Some(reason) = status.reason {
        debug!("reason: {}", reason);
    }
    Ok(status.allowed)
}

/// CLI way to resolve kube context
///
/// Should only be used from main.
pub async fn current_context() -> Result<String> {
    let (mut res, _) = kout(vec!["config".into(), "current-context".into()])
        .await
        .map_err(|e| {
            error!("Failed to Get kubectl config current-context. Is kubectl installed?");
            e
        })?;
    let len = res.len();
    if res.ends_with('\n') {
        res.truncate(len - 1);
    }
    Ok(res)
}

pub async fn set_context(context: &str, args: Vec<String>) -> Result<String> {
    let mut arg_list = vec!["config".into(), "set-context".into(), context.into()];
    arg_list.extend_from_slice(&args);

    let (res, _) = kout(arg_list).await.map_err(|e| {
        error!("Failed to set kubectl config set-context. Is kubectl installed?");
        e
    })?;

    Ok(res)
}

pub async fn use_context(context: &str) -> Result<String> {
    let (res, _) = kout(vec!["config".into(), "use-context".into(), context.into()])
        .await
        .map_err(|e| {
            error!("Failed to set kubectl config use-context. Is kubectl installed?");
            e
        })?;

    Ok(res)
}

/// Shell into a pod associated with a workload
pub async fn shell(mf: &Manifest, cmd: Option<Vec<&str>>) -> Result<()> {
    // TODO: kubectl auth can-i create pods/exec

    let target = format!("{}/{}", mf.workload.to_string(), mf.name);
    debug!("Shelling into {}", target);

    // kubectl exec -it deployment/$pod sh
    let mut execargs = vec![
        "exec".into(),
        format!("-n={}", mf.namespace),
        "-it".into(),
        target.clone(),
    ];
    if let Some(cmdu) = cmd.clone() {
        for c in cmdu {
            execargs.push(c.into())
        }
    } else {
        let trybash = vec![
            "exec".into(),
            format!("-n={}", mf.namespace),
            target,
            "which".into(),
            "bash".into(),
        ];
        // kubectl exec $pod which bash
        // returns a non-zero rc if not found generally
        let shexe = match kexec(trybash).await {
            Ok(o) => {
                debug!("Got {:?}", o);
                "bash".into()
            }
            Err(e) => {
                warn!("No bash in container, falling back to `sh`");
                debug!("Error: {}", e);
                "sh".into()
            }
        };
        execargs.push(shexe);
    }
    kexec(execargs).await?;
    Ok(())
}

/// Port forward a port to localhost
///
/// Useful because we have autocomplete on manifest names in shipcat
pub async fn port_forward(mf: &Manifest) -> Result<()> {
    let access_request = AccessReviewRequest {
        namespace: mf.namespace.clone(),
        verb: "create".into(),
        resource: "pods".into(),
        subresource: Some("portforward".into()),
    };

    if !kani(access_request).await? {
        bail!("Current token does not have authorization to port-forward")
    };

    // TODO: kubectl auth can-i create pods/portforward first
    let port_offset = 7777;
    let mut ps: Vec<_> = mf.ports.iter().map(|mp| mp.port).collect();

    if let Some(p) = mf.httpPort {
        if ps.iter().find(|&&mp| mp == p).is_none() {
            ps.push(p);
        }
    };

    if ps.is_empty() {
        bail!("{} does not expose any port to port-forward to", mf.name)
    };

    let ports = ps.iter().enumerate().map(|(i, &port)| {
        let localport: u32 = if port <= 1024 {
            port_offset + i as u32
        } else {
            port
        };
        debug!(
            "Port forwarding kube deployment {}:{} to localhost:{}",
            mf.name, port, localport
        );
        (port, localport)
    });

    // kubectl port-forward deployment/${name} localport:httpPort
    let mut pfargs = vec![
        format!("-n={}", mf.namespace),
        "port-forward".into(),
        format!("{}/{}", mf.workload.to_string(), mf.name),
    ];

    for (port, localport) in ports {
        pfargs.push(format!("{}:{}", localport, port));
    }

    kexec(pfargs).await?;
    Ok(())
}

/// Apply the kube object an applyable file
///
/// CRDs itself, Manifest and Config typically.
/// Returns whether or not the file was configured
pub async fn apply_resource<K: k8s_openapi::Resource + Serialize>(
    name: &str,
    data: K,
    ns: &str,
) -> Result<bool> {
    use std::{
        fs::{self, File},
        io::Write,
        path::Path,
    };

    // Write it to a temporary file:
    let datafile = format!("{}.crd.gen.yml", name);
    let pth = Path::new(".").join(&datafile);
    debug!("Writing {} CRD for {} to {}", K::KIND, name, pth.display());
    let mut f = File::create(&pth)?;
    let encoded = serde_yaml::to_string(&data)?;
    writeln!(f, "{}", encoded)?;
    debug!(
        "Wrote {} CRD for {} to {}: \n{}",
        K::KIND,
        name,
        pth.display(),
        encoded
    );

    // Apply it using kubectl apply
    debug!("Applying {} CRD for {}", K::KIND, name);
    let applyargs = vec![
        format!("-n={}", ns),
        "apply".into(),
        "-f".into(),
        datafile.clone(),
    ];
    debug!("applying {} : {:?}", name, applyargs);
    let (out, status) = kout(applyargs.clone()).await?;
    print!("{}", out); // always print kube output from this
    if !status {
        bail!("subprocess failure from kubectl: {:?}", applyargs);
    }
    let changed = if out.contains("configured") || out.contains("created") {
        true
    } else if out.contains("unchanged") {
        false
    } else {
        bail!("unrecognized apply result: {}", out)
    };
    let _ = fs::remove_file(&datafile); // try to remove temporary file
    Ok(changed)
}
/// Find all ManifestCrds in a given namespace
///
/// Allows us to purge manifests that are not in Manifest::available()
async fn find_all_manifest_crds(ns: &str) -> Result<Vec<String>> {
    let getargs = vec![
        "get".into(),
        format!("-n={}", ns),
        "shipcatmanifests".into(),
        "-ojsonpath='{.items[*].metadata.name}'".into(),
    ];
    let (out, _) = kout(getargs).await?;
    if out == "''" {
        // stupid kubectl
        return Ok(vec![]);
    }
    Ok(out.split(' ').map(String::from).collect())
}

use std::path::PathBuf;
// Kubectl diff experiment (ignores secrets)
pub async fn diff(pth: PathBuf, ns: &str) -> Result<(String, String, bool)> {
    let args = vec![
        "diff".into(),
        format!("-n={}", ns),
        format!("-f={}", pth.display()),
    ];
    // need the error code here so re-implent - and discard stderr
    debug!("kubectl {}", args.join(" "));

    let s = Command::new("kubectl").args(&args).output().await?;
    let out: String = String::from_utf8_lossy(&s.stdout).into();
    let err: String = String::from_utf8_lossy(&s.stderr).into();
    trace!("out: {}, err: {}", out, err);
    if err.contains("the dryRun alpha feature is disabled") {
        bail!("kubectl diff is not supported in your cluster: {}", err.trim());
    }
    Ok((out, err, s.status.success()))
}

pub async fn find_redundant_manifests(ns: &str, svcs: &[String]) -> Result<Vec<String>> {
    use std::collections::HashSet;
    let requested: HashSet<_> = svcs.iter().cloned().collect();
    let found: HashSet<_> = find_all_manifest_crds(ns).await?.iter().cloned().collect();
    debug!("Found manifests: {:?}", found);
    Ok(found.difference(&requested).cloned().collect())
}

// Get a version of a service from the current shipcatmanifest crd
pub async fn get_running_version(svc: &str, ns: &str) -> Result<String> {
    // kubectl get shipcatmanifest $* -o jsonpath='{.spec.version}'
    let mfargs = vec![
        "get".into(),
        "shipcatmanifest".into(),
        svc.into(),
        format!("-n={}", ns),
        "-ojsonpath='{.spec.version}'".into(),
    ];
    match kout(mfargs).await {
        Ok((kout, true)) => Ok(kout),
        _ => bail!("Manifest for '{}' not found in {}", svc, ns),
    }
}

#[cfg(test)]
mod tests {
    use super::{current_context, get_running_version};
    use dirs;

    #[tokio::test]
    async fn validate_ctx() {
        let kubecfg = dirs::home_dir().unwrap().join(".kube").join("config");
        // ignoring this test on circleci..
        if kubecfg.is_file() {
            let ctx = current_context().await.unwrap();
            assert_eq!(ctx, ctx.trim());
            assert_ne!(ctx, "");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn check_get_version() {
        let r = get_running_version("raftcat", "dev").await.unwrap();
        assert_eq!(r, "0.121.0");
    }
}
