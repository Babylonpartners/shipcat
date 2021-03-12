#![recursion_limit = "1024"]
#![allow(renamed_and_removed_lints)]
#![allow(non_snake_case)]
#![warn(rust_2018_idioms)]

#[macro_use] extern crate serde_derive;
#[macro_use] extern crate log;

#[macro_use] extern crate error_chain;

error_chain! {
    types {
        Error, ErrorKind, ResultExt, Result;
    }
    links {}
    foreign_links {
        Fmt(::std::fmt::Error);
        Io(::std::io::Error) #[cfg(unix)];
        Float(::std::num::ParseFloatError);
        Int(::std::num::ParseIntError);
        TryInt(::std::num::TryFromIntError);
        Mani(shipcat_definitions::Error);
        SerdeY(serde_yaml::Error);
        SerdeJ(serde_json::Error);
        Reqe(reqwest::Error);
        UrlP(url::ParseError);
        Slack(slack_hook2::SlackError);
        Time(::std::time::SystemTimeError);
        Chrono(chrono::format::ParseError);
    }
    errors {
        MissingSlackUrl {
            description("SLACK_SHIPCAT_HOOK_URL not specified")
            display("SLACK_SHIPCAT_HOOK_URL not specified")
        }
        MissingSlackChannel {
            description("SLACK_SHIPCAT_CHANNEL not specified")
            display("SLACK_SHIPCAT_CHANNEL not specified")
        }
        Url(url: reqwest::Url) {
            description("could not access URL")
            display("could not access URL '{}'", &url)
        }
        InvalidManifest(svc: String) {
            description("invalid manifest")
            display("{} failed validation", &svc)
        }
        MissingRollingVersion(svc: String) {
            description("missing version for install")
            display("{} has no version in manifest and is not installed yet", &svc)
        }
        ManifestFailure(key: String) {
            description("Manifest key not propagated correctly internally")
            display("manifest key {} was not propagated internally - bug!", &key)
        }
        HelmUpgradeFailure(svc: String) {
            description("Helm upgrade call failed")
            display("Helm upgrade of {} failed", &svc)
        }
        KubectlApplyFailure(svc: String) {
            description("Kube apply call failed")
            display("Kube apply of {} failed", &svc)
        }
        KubectlApiFailure(call: String, svc: String) {
            description("kube call failed")
            display("kube {} of {} failed", &call, &svc)
        }
        UpgradeTimeout(svc: String, secs: u32) {
            description("upgrade timed out")
            display("{} upgrade timed out waiting {}s for deployment(s) to come online", &svc, secs)
        }
        SlackSendFailure(hook: String) {
            description("slack message send failed")
            display("Failed to send the slack message to '{}' ", &hook)
        }
        KubeError(e: kube::Error) {
            description("kube api interaction failed")
            display("kube api: {}: {:?}", e, e)
        }
        SelfUpgradeError(s: String) {
            description("self-upgrade failed")
            display("self-upgrade: {}", s)
        }
    }
}

pub use shipcat_definitions::{
    config::{self, Config, ConfigFallback},
    region::{AuditWebhook, KongConfig, Region, VersionScheme, Webhook},
    structs, ConfigState, Manifest,
};
// pub use shipcat_definitions::Product;

/// Audit objects and API caller
pub mod audit;
/// Cluster level operations
pub mod cluster;
/// Convenience listers
pub mod list;
/// A post interface to slack using `slack_hook`
pub mod slack;

/// Validation methods of manifests post merge
pub mod validate;

/// gdpr lister
pub mod gdpr;

/// A small CLI kubernetes interface
pub mod kubectl;

/// A newer API kubernetes interface
pub mod kubeapi;

/// A newer upgrade tracking interface
pub mod track;

/// Status subcommand
pub mod status;

/// Apply logic
pub mod apply;

/// A small CLI helm template interface
pub mod helm;

/// A small CLI kong config generator interface
pub mod kong;

/// A small CLI Statuscake config generator interface
pub mod statuscake;

/// A graph generator for manifests using `petgraph`
pub mod graph;

/// Various simple reducers
pub mod get;

/// Top resource use
pub mod top;
pub use top::{OutputFormat, ResourceOrder};

/// Diffing module for values
pub mod diff;

/// Git stuff
pub mod git;

/// Env module for sourcing secrets
pub mod env;

/// Webhook mux/demux
pub mod webhooks;
pub use webhooks::UpgradeState;

/// Simple printers
pub mod show;

/// Cluster auth
pub mod auth;

/// Shipcat self upgrade
#[cfg(feature = "self-upgrade")]
pub mod upgrade;

/// Smart initialiser with safety
///
/// Tricks the library into reading from your manifest location.
pub fn init() -> Result<()> {
    use std::{env, path::Path};

    // Allow shipcat calls to work from anywhere if we know where manifests are
    if let Ok(mdir) = env::var("SHIPCAT_MANIFEST_DIR") {
        let pth = Path::new(&mdir);
        if !pth.is_dir() {
            bail!("SHIPCAT_MANIFEST_DIR must exist");
        }
        env::set_current_dir(pth)?;
    }

    Ok(())
}
