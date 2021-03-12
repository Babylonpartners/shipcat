use std::fmt;

use shipcat_definitions::{structs::Kong, BaseManifest};

/// Simplified Manifest for a specific region (no templating/config files loaded).
pub struct SimpleManifest {
    pub base: BaseManifest,
    pub region: String,

    /// Is the service enabled in the current region?
    pub enabled: bool,
    /// Is the service external?
    pub external: bool,
    pub version: Option<String>,
    pub image: Option<String>,
    pub kong_apis: Vec<Kong>,
}

impl fmt::Debug for SimpleManifest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "SimpleManifest {{ name: {}, region: {} }}",
            self.base.name, self.region
        )
    }
}
