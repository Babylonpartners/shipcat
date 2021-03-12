use super::Result;

/// ConfigMap
///
/// A special abstraction that is used to create a kubernetes ConfigMap
/// Deals with automatic mounting into the pods.
///
/// Only one of these is supported.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct ConfigMap {
    /// Container-local directory path where configs are available
    pub mount: String,
    /// Files from the config map to mount at this mountpath
    pub files: Vec<ConfigMappedFile>,
}

/// ConfigMapped File
///
/// Files that are mounted under the parent `mount` path.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct ConfigMappedFile {
    /// Name of file to template (from service repo paths)
    pub name: String,
    /// Name of file inside container
    pub dest: String,
    /// Config value inlined
    ///
    /// This is usually filled in internally by to help out Helm a bit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl ConfigMap {
    pub fn verify(&self) -> Result<()> {
        // mount paths can't be empty string
        if self.mount == "" || self.mount.starts_with('~') {
            bail!("Invalid mountpath '{}'", self.mount)
        }
        // and must end in a slash to have a standard
        if !self.mount.ends_with('/') {
            bail!("Mount path '{}' must end with a slash", self.mount);
        }
        for f in &self.files {
            if !f.name.ends_with(".j2") {
                bail!("Only supporting templated config files atm")
            }
            if f.dest == "" {
                bail!("Empty mount destination for {}", f.name);
            }
        }
        // TODO: verify file exists? done later anyway
        Ok(())
    }
}
