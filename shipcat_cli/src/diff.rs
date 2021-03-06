use super::{Config, ConfigState, Manifest, Region, Result};
use crate::{git, helm, kubectl};
use regex::Regex;
use shipcat_definitions::ShipcatManifest;
use std::process::Command;

/// YAML serialisation of a manifest.
///
/// Return an empty string if the manifest fails region-validation,
/// otherwise YAML serialise the content. For diff purposes, the content
/// of a manifest not in a region is a blank, rather than being invalid.
async fn as_yaml(svc: &str, conf: &Config, region: &Region) -> Result<String> {
    let mf = shipcat_filebacked::load_manifest(&svc, conf, region).await?;
    if let Ok(m) = mf.verify_region() {
        let yaml = serde_yaml::to_string(&m)?;
        Ok(yaml)
    } else {
        Ok("".to_string())
    }
}

/// Fast local git compare of the crd
///
/// Should be pretty safe. Stashes existing work, checks out master, compares,
/// then goes back to previous branch and pops the stash.
///
/// Because this does fiddle with git state while running it is not the default implementation.
pub async fn values_vs_git(svc: &str, conf: &Config, region: &Region) -> Result<bool> {
    let after = as_yaml(&svc, conf, region).await?;

    // move git to get before state:
    let merge_base = git::merge_base()?;
    git::checkout(&merge_base)?;

    let needs_stash = git::needs_stash();
    if needs_stash {
        git::stash_push()?;
    }

    // compute before state
    let (before_conf, before_region) = Config::new(ConfigState::Base, &region.name).await?;
    let before = as_yaml(&svc, &before_conf, &before_region).await?;

    // move git back
    if needs_stash {
        git::stash_pop()?;
    }
    git::checkout("-")?;

    // display diff
    shell_diff(&before, &after, "before", "after")
}

/// Fast local compare of shipcat template for two regions
pub async fn values_vs_region(
    svc: &str,
    conf: &Config,
    region: &Region,
    ref_region: &Region,
) -> Result<bool> {
    let before_region = format!("{}.{}", svc, ref_region.name);
    let before_values = as_yaml(svc, conf, ref_region).await?;

    let after_region = format!("{}.{}", svc, region.name);
    let after_values = as_yaml(svc, conf, region).await?;

    // display diff
    shell_diff(&before_values, &after_values, &before_region, &after_region)
}

/// Fast local git compare of shipcat template
///
/// Because this uses the template in master against local state,
/// we don't resolve secrets for this (would compare equal values anyway).
pub async fn template_vs_git(svc: &str, conf: &Config, region: &Region) -> Result<bool> {
    let afterpth = Path::new(".").join("after.shipcat.gen.yml");
    let mf_after = shipcat_filebacked::load_manifest(svc, conf, region)
        .await?
        .stub(region)
        .await?;
    let _after = helm::template(&mf_after, Some(afterpth.clone())).await?;

    // move git to get before state:
    let merge_base = git::merge_base()?;
    git::checkout(merge_base.as_str())?;

    let needs_stash = git::needs_stash();
    if needs_stash {
        git::stash_push()?;
    }

    // compute old state:
    let (before_conf, before_region) = Config::new(ConfigState::Base, &region.name).await?;

    let beforepth = Path::new(".").join("before.shipcat.gen.yml");
    let mf_before = shipcat_filebacked::load_manifest(svc, &before_conf, &before_region)
        .await?
        .stub(region)
        .await?;
    let _before = helm::template(&mf_before, Some(beforepth.clone())).await?;

    // move git back
    if needs_stash {
        git::stash_pop()?;
    }
    git::checkout("-")?;

    // display diff
    // doesn't reuse shell_diff because we already have files from direct::template
    let args = ["-u", "before.shipcat.gen.yml", "after.shipcat.gen.yml"];
    debug!("diff {}", args.join(" "));
    let s = Command::new("diff").args(&args).status()?;
    // cleanup
    fs::remove_file(beforepth)?;
    fs::remove_file(afterpth)?;
    Ok(s.success())
}

use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

/// Diff values using kubectl diff
///
/// Generate crd as we write it and pipe it to `kubectl diff -`
/// Only works on clusters with kubectl 1.13 on the server side, so not available everywhere
pub async fn values_vs_kubectl(svc: &str, conf: &Config, region: &Region) -> Result<bool> {
    // Generate crd in a temp file:
    let mf = shipcat_filebacked::load_manifest(svc, conf, region).await?;
    let crd = ShipcatManifest::from(mf);
    let encoded = serde_yaml::to_string(&crd)?;
    let cfile = format!("{}.shipcat.crd.gen.yml", svc);
    let pth = Path::new(".").join(cfile);
    debug!("Writing crd for {} to {}", svc, pth.display());
    let mut f = File::create(&pth)?;
    writeln!(f, "{}", encoded)?;
    // shell out to kubectl:
    let (out, _err, success) = kubectl::diff(pth.clone(), &region.namespace).await?;
    println!("{}", out);
    // cleanup:
    fs::remove_file(pth)?;
    Ok(success)
}

/// Diff using template kubectl diff
///
/// Generate template as we write it and pipe it to `kubectl diff -`
/// Only works on clusters with kubectl 1.13 on the server side, so not available everywhere
pub async fn template_vs_kubectl(mf: &Manifest) -> Result<Option<String>> {
    // Generate template in a temp file:
    let tfile = format!("{}.shipcat.tpl.gen.yml", mf.name);
    let pth = Path::new(".").join(tfile);

    let _tpl = helm::template(&mf, Some(pth.clone())).await?;

    let (out, err, success) = kubectl::diff(pth.clone(), &mf.namespace).await?;
    // cleanup:
    fs::remove_file(pth)?;
    if !success && !err.is_empty() && err.trim() != "exit status 1" {
        println!("kubectl diff stderr: {}", err.trim());
    }
    if !out.is_empty() {
        Ok(Some(out))
    } else {
        Ok(None)
    }
}

// Compare using diff(1)
// difference libraries all seemed to be lacking somewhat
fn shell_diff(before: &str, after: &str, before_name: &str, after_name: &str) -> Result<bool> {
    let beforefilename = format!("{}.shipcat.gen.yml", before_name);
    let beforepth = Path::new(".").join(&beforefilename);
    debug!("Writing before to {}", beforepth.display());
    let mut f = File::create(&beforepth)?;
    writeln!(f, "{}", before)?;

    let afterfilename = format!("{}.shipcat.gen.yml", after_name);
    let afterpth = Path::new(".").join(&afterfilename);
    debug!("Writing after to {}", afterpth.display());
    let mut f = File::create(&afterpth)?;
    writeln!(f, "{}", after)?;

    let args = ["-u", &beforefilename, &afterfilename];
    debug!("diff {}", args.join(" "));
    let s = Command::new("diff").args(&args).status()?;
    // cleanup
    fs::remove_file(beforepth)?;
    fs::remove_file(afterpth)?;

    Ok(s.success())
}

/// Minify diff output from kubectl diff
pub fn minify(diff: &str) -> String {
    let minusplus = Regex::new(r"^\- |^\+ ").unwrap();
    let generation = Regex::new(r"generation[:]{1}").unwrap();
    let kind_line = Regex::new(r"--- /tmp/LIVE-[a-zA-Z0-9]+/([\w\.]+)").unwrap();
    // Find the +++/--- header and extract the type from it.
    // Then trim everything that doesn't start with `- ` or `+ `
    // and additionally ignore `generation` integer updates

    let mut res = vec![];
    let mut in_secret = false;
    for l in diff.lines() {
        if let Some(cap) = kind_line.captures(l) {
            in_secret = cap[1].contains("Secret");
            if in_secret {
                res.push(format!("Change to {} elided for security", &cap[1]));
            } else {
                res.push(format!("{} has changed:", &cap[1]));
            }
        } else if !in_secret && !l.starts_with("+++") && !generation.is_match(l) && minusplus.is_match(l) {
            res.push(l.to_string());
        }
    }
    res.join("\n")
}

/// Check if a diff contains only version related changes
pub fn is_version_only(diff: &str, vers: (&str, &str)) -> bool {
    let smalldiff = minify(diff);
    trace!("Checking diff for {:?}", vers);
    for l in smalldiff.lines() {
        // ignore headline for resource type (no actual changes)
        if !l.starts_with('+') && !l.starts_with('-') && l.contains("has changed:") {
            continue;
        }
        // ignore all lines that contain one of the versions
        if l.contains(vers.0) || l.contains(vers.1) {
            continue;
        }
        // any other lines found => not just a version change
        return false;
    }
    true
}

/// Infer a version change diff and extract old version and new version
pub fn infer_version_change(diff: &str) -> Option<(String, String)> {
    let img_re = Regex::new(r"[^:]+:(?P<version>[a-z0-9\.\-]+)").unwrap();
    let res = img_re
        .captures_iter(diff)
        .map(|cap| cap["version"].to_string())
        .collect::<Vec<String>>();
    if res.len() >= 2 {
        return Some((res[0].clone(), res[1].clone()));
    }
    None
}

/// Obfuscate a set of secrets from an input string
pub fn obfuscate_secrets(input: String, secrets: Vec<String>) -> String {
    let mut out = input;
    for s in secrets {
        // If your secret is less than 8 characters, we won't obfuscate it
        // Mostly for fear of clashing with other parts of the output,
        // but also because it's an insecure secret anyway
        if s.len() >= 8 {
            out = out.replace(&s, "************");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{infer_version_change, is_version_only, minify};

    #[test]
    fn version_change_test() {
        let input = "pa-aggregator, Deployment (apps/v1) has changed:
-         image: \"quay.io/babylonhealth/pa-aggregator-python:e7c1e5dd5de74b2b5da5eef76eb5bf12bdc2ac19\"
+         image: \"quay.io/babylonhealth/pa-aggregator-python:d4f01f5143643e75d9cc2d5e3221e82a9e1c12e5\"";
        let res = infer_version_change(input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert_eq!(old, "e7c1e5dd5de74b2b5da5eef76eb5bf12bdc2ac19");
        assert_eq!(new, "d4f01f5143643e75d9cc2d5e3221e82a9e1c12e5");
    }

    #[test]
    fn version_change_semver() {
        let input = "pa-aggregator, Deployment (apps/v1) has changed:
-         image: \"quay.io/babylonhealth/pa-aggregator-python:1.2.3\"
+         image: \"quay.io/babylonhealth/pa-aggregator-python:1.3.0-alpine\"";
        let res = infer_version_change(input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert_eq!(old, "1.2.3");
        assert_eq!(new, "1.3.0-alpine");
    }

    #[test]
    fn version_diff_test() {
        // simple version change with versions referenced more than once
        let input = "react-ask-frontend, Deployment (apps/v1) has changed:
-         image: \"quay.io/babylonhealth/react-ask-frontend:6418d7cacb7438ddd4e533d78b38902bc7f79e7b\"
+         image: \"quay.io/babylonhealth/react-ask-frontend:d27b5c6f96f05436b236dae112c7c8fcedca4c71\"
-           value: 6418d7cacb7438ddd4e533d78b38902bc7f79e7b
+           value: d27b5c6f96f05436b236dae112c7c8fcedca4c71";

        let res = infer_version_change(input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert_eq!(old, "6418d7cacb7438ddd4e533d78b38902bc7f79e7b");
        assert_eq!(new, "d27b5c6f96f05436b236dae112c7c8fcedca4c71");
        assert!(is_version_only(input, (&new, &old)));
    }

    #[test]
    fn version_diff_test2() {
        // not just a simple version change
        let input = "react-ask-frontend, Deployment (apps/v1) has changed:
-         image: \"quay.io/babylonhealth/react-ask-frontend:6418d7cacb7438ddd4e533d78b38902bc7f79e7b\"
+         image: \"quay.io/babylonhealth/react-ask-frontend:d27b5c6f96f05436b236dae112c7c8fcedca4c71\"
-           blast: keyremoval";
        let res = infer_version_change(input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert_eq!(old, "6418d7cacb7438ddd4e533d78b38902bc7f79e7b");
        assert_eq!(new, "d27b5c6f96f05436b236dae112c7c8fcedca4c71");
        assert!(!is_version_only(input, (&new, &old)));
    }

    #[test]
    fn version_diff_test3() {
        // semver version change
        let input = "knowledge-base2-search, Deployment (apps/v1) has changed:
-         image: \"quay.io/babylonhealth/knowledgebase2:1.0.6\"
+         image: \"quay.io/babylonhealth/knowledgebase2:1.0.7\"
-           value: 1.0.6
+           value: 1.0.7";
        let res = infer_version_change(input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert_eq!(old, "1.0.6");
        assert_eq!(new, "1.0.7");
        assert!(is_version_only(input, (&new, &old)));
    }

    #[test]
    fn kubectl_diff_minify_test() {
        let input = "--- /tmp/LIVE-A9/apps.v1.Deployment.dev.raftcat   2019-09-11 16:12:26.819641578 +0100
+++ /tmp/MERGED-B0/apps.v1.Deployment.dev.raftcat 2019-09-11 16:12:26.852974183 +0100
@@ -6,7 +6,7 @@
     kubectl.kubernetes.io/last-applied-configuration: |
       {\"apiVersion\":\"apps/v1\",\"kind\":\"Deployment\"}
   creationTimestamp: \"2019-09-11T14:49:14Z\"
-  generation: 5
+  generation: 6
   labels:
     app: raftcat
     chart: raftcat-0.3.0
@@ -66,7 +66,7 @@
       containers:
       - env:
         - name: BLAAA
-          value: eirik4
+          value: eirik5
         - name: LOG_LEVEL
           value: DEBUG
         - name: NAMESPACE";

        assert_eq!(
            minify(input),
            "apps.v1.Deployment.dev.raftcat has changed:
-          value: eirik4
+          value: eirik5"
        );
    }

    #[test]
    fn kubectl_diff_version_only() {
        let min_input = "extensions.v1beta1.Deployment.dev has changed:
-    app.kubernetes.io/version: a844d0db93216b25d22a482ab80029d4a552f285
+    app.kubernetes.io/version: 203894776eed17f00b9dd0bc25a09dcef644ea67
-          value: a844d0db93216b25d22a482ab80029d4a552f285
-        image: quay.io/babylonhealth/aim-dashboard:a844d0db93216b25d22a482ab80029d4a552f285
+          value: 203894776eed17f00b9dd0bc25a09dcef644ea67
+        image: quay.io/babylonhealth/aim-dashboard:203894776eed17f00b9dd0bc25a09dcef644ea67
v1.ServiceAccount.dev has changed:
-    app.kubernetes.io/version: a844d0db93216b25d22a482ab80029d4a552f285
+    app.kubernetes.io/version: 203894776eed17f00b9dd0bc25a09dcef644ea67
v1.Service.dev has changed:
-    app.kubernetes.io/version: a844d0db93216b25d22a482ab80029d4a552f285
+    app.kubernetes.io/version: 203894776eed17f00b9dd0bc25a09dcef644ea67";

        let res = infer_version_change(min_input);
        assert!(res.is_some());
        let (old, new) = res.unwrap();
        assert!(is_version_only(min_input, (&old, &new)));
    }

    #[test]
    fn kubectl_diff_secret_squash_test() {
        // A pretty large diff input that checks that secrets are filtered out
        // ..and that it doesn't filter out objects surrounding the secrets
        let input = r#"diff -u -N /tmp/LIVE-547038353/apps.v1.Deployment.dev.raftcat /tmp/MERGED-191875772/apps.v1.Deployment.dev.raftcat
--- /tmp/LIVE-547038353/apps.v1.Deployment.dev.raftcat   2020-04-07 12:25:26.255493075 +0100
+++ /tmp/MERGED-191875772/apps.v1.Deployment.dev.raftcat 2020-04-07 12:25:26.312159054 +0100
@@ -6,7 +6,7 @@
   creationTimestamp: "2019-11-18T22:47:05Z"
-  generation: 227
+  generation: 228
   labels:
     app: raftcat
     app.kubernetes.io/managed-by: shipcat
@@ -45,7 +45,7 @@
     metadata:
       annotations:
         checksum/config: 01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b
-        checksum/secrets: 3e6c8c2098228c15554026bef845a1e121c6d32a315ecbba953d92e8b4d26bed
+        checksum/secrets: fd7f339fc814c7067c66213705ff7bfc78a81723e50c1083bd3a29df272d34e7
         kubectl.kubernetes.io/restartedAt: "2020-03-18T18:40:01Z"
       creationTimestamp: null
diff -u -N /tmp/LIVE-422759316/v1.Secret.dev.raftcat-secrets /tmp/MERGED-101659107/v1.Secret.dev.raftcat-secrets
--- /tmp/LIVE-422759316/v1.Secret.dev.raftcat-secrets   2020-04-07 12:26:32.618020569 +0100
+++ /tmp/MERGED-101659107/v1.Secret.dev.raftcat-secrets 2020-04-07 12:26:32.664686669 +0100
@@ -1,9 +1,10 @@
 apiVersion: v1
 data:
-  SENTRY_DSN: aGVsbG8gd29ybGQK==
+  SENTRY_DSN: YUdWc2JHOGdkMjl5YkdRPQ==
+  WOOT: aGk=
 kind: Secret
 metadata:
diff -u -N /tmp/LIVE-167159470/autoscaling.v2beta2.HorizontalPodAutoscaler.dev.raftcat /tmp/MERGED-264369205/autoscaling.v2beta2.HorizontalPodAutoscaler.dev.raftcat
--- /tmp/LIVE-167159470/autoscaling.v2beta2.HorizontalPodAutoscaler.dev.raftcat 2020-04-07 13:02:15.788649534 +0100
+++ /tmp/MERGED-264369205/autoscaling.v2beta2.HorizontalPodAutoscaler.dev.raftcat   2020-04-07 13:02:15.875315149 +0100
@@ -22,7 +22,7 @@
 spec:
-  maxReplicas: 3
+  maxReplicas: 4
   metrics:
   - resource:
       name: cpu"#;

        assert_eq!(
            minify(input),
            "apps.v1.Deployment.dev.raftcat has changed:
-        checksum/secrets: 3e6c8c2098228c15554026bef845a1e121c6d32a315ecbba953d92e8b4d26bed
+        checksum/secrets: fd7f339fc814c7067c66213705ff7bfc78a81723e50c1083bd3a29df272d34e7
Change to v1.Secret.dev.raftcat elided for security
autoscaling.v2beta2.HorizontalPodAutoscaler.dev.raftcat has changed:
-  maxReplicas: 3
+  maxReplicas: 4"
        );
    }
}
