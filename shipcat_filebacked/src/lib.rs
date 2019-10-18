#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate merge_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate error_chain;

// Structs
mod authorization;
mod container;
mod manifest;
mod newrelic_source;
mod sentry_source;
mod simple;
pub use crate::simple::SimpleManifest;
mod kong;

mod load;
mod util;

use manifest::ManifestSource;
use shipcat_definitions::{BaseManifest, Config, Manifest, Region, Result};

pub fn load_manifest(service: &str, conf: &Config, reg: &Region) -> Result<Manifest> {
    ManifestSource::load_manifest(service, conf, reg)
}

pub fn load_metadata(service: &str, conf: &Config, reg: &Region) -> Result<SimpleManifest> {
    ManifestSource::load_metadata(service, conf, reg)
}

pub fn all(conf: &Config) -> Result<Vec<BaseManifest>> {
    ManifestSource::all(conf)
}

pub fn available(conf: &Config, reg: &Region) -> Result<Vec<SimpleManifest>> {
    ManifestSource::available(conf, reg)
}
