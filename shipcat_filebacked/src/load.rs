use std::path::{Path, PathBuf};

use merge::Merge;
use serde::de::DeserializeOwned;
use shipcat_definitions::{Config, ErrorKind, Manifest, Region, Result, ResultExt};
use walkdir::WalkDir;

use super::{BaseManifest, SimpleManifest};
use crate::manifest::{ManifestDefaults, ManifestOverrides, ManifestSource};

impl ManifestSource {
    pub async fn load_manifest(service: &str, conf: &Config, reg: &Region) -> Result<Manifest> {
        let reg_name = reg.name.clone();
        let service_name = service.to_string();

        let merged = ManifestSource::load_merged(service, conf, reg)
            .await
            .chain_err(|| ErrorKind::FailedToBuildManifest(service_name.clone(), reg_name.clone()))?;
        merged
            .build(&(conf.clone(), reg.clone()))
            .await
            .chain_err(|| ErrorKind::FailedToBuildManifest(service_name.clone(), reg_name.clone()))
    }

    pub async fn load_metadata(service: &str, conf: &Config, reg: &Region) -> Result<SimpleManifest> {
        let manifest = ManifestSource::load_merged(service, conf, reg).await?;
        manifest.build_simple(&conf, &reg)
    }

    async fn load_merged(service: &str, conf: &Config, reg: &Region) -> Result<Self> {
        let dir = Self::services_dir().join(service);

        if !dir.exists() {
            bail!("Service folder {} does not exist", dir.display())
        }

        let global_defaults = ManifestDefaults::from_global(conf)?;
        let regional_defaults = ManifestDefaults::from_region(reg)?;
        let defaults = global_defaults.merge(regional_defaults);

        let source_path = Self::services_dir().join(service).join("manifest.yml");
        debug!("Loading service manifest from {:?}", source_path);
        let source: ManifestSource = read_from(&source_path).await?;
        let mut manifest = defaults.merge_source(source);

        let env_path = dir.join(format!("{}.yml", reg.environment.to_string()));
        if env_path.is_file() {
            debug!("Loading service overrides from {:?}", env_path);
            let env: ManifestOverrides = read_from(&env_path).await?;
            manifest = manifest.merge_overrides(env);
        }

        let region_path = dir.join(format!("{}.yml", reg.name));
        if region_path.is_file() {
            debug!("Loading service overrides from {:?}", region_path);
            let region: ManifestOverrides = read_from(&region_path).await?;
            manifest = manifest.merge_overrides(region);
        }

        Ok(manifest)
    }

    fn all_names() -> Vec<String> {
        let mut res: Vec<_> = WalkDir::new(&ManifestSource::services_dir())
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
            .map(|e| {
                let mut cmps = e.path().components();
                cmps.next(); // .
                cmps.next(); // services
                let svccomp = cmps.next().unwrap();
                let svcname = svccomp.as_os_str().to_str().unwrap();
                svcname.to_string()
            })
            .collect();
        res.sort();
        res
    }

    pub async fn all(conf: &Config) -> Result<Vec<BaseManifest>> {
        let mut all = vec![];
        for service in Self::all_names() {
            let source_path = Self::services_dir().join(&service).join("manifest.yml");
            debug!("Loading service manifest from {:?}", source_path);
            let source: ManifestSource = read_from(&source_path)
                .await
                .chain_err(|| ErrorKind::InvalidManifest(service.clone()))?;
            let manifest = source
                .build_base(conf)
                .chain_err(|| ErrorKind::InvalidManifest(service.clone()))?;
            all.push(manifest);
        }
        Ok(all)
    }

    pub async fn available(conf: &Config, reg: &Region) -> Result<Vec<SimpleManifest>> {
        let mut available = vec![];
        for service in Self::all_names() {
            let manifest = Self::load_metadata(&service, conf, reg)
                .await
                .chain_err(|| ErrorKind::InvalidManifest(service.clone()))?;
            if manifest.enabled && !manifest.external {
                available.push(manifest);
            }
        }
        Ok(available)
    }

    fn services_dir() -> PathBuf {
        Path::new(".").join("services")
    }
}

impl ManifestDefaults {
    fn from_global(conf: &Config) -> Result<Self> {
        match serde_yaml::from_value(conf.defaults.clone()) {
            Err(e) => bail!("Global defaults did not parse as YAML: {}", e),
            Ok(d) => Ok(d),
        }
    }

    fn from_region(reg: &Region) -> Result<Self> {
        match serde_yaml::from_value(reg.defaults.clone()) {
            Err(e) => bail!("Region {} defaults did not parse as YAML: {}", reg.name, e),
            Ok(d) => Ok(d),
        }
    }
}

async fn read_from<T: DeserializeOwned>(path: &PathBuf) -> Result<T> {
    use tokio::fs;
    trace!("Reading manifest in {}", path.display());
    if !path.exists() {
        bail!("Manifest file {} does not exist", path.display())
    }
    let data = fs::read_to_string(&path).await?;
    if data.is_empty() {
        bail!("Manifest file {} is empty", path.display());
    }
    match serde_yaml::from_str(&data) {
        Err(e) => bail!("Manifest file {} did not parse as YAML: {}", path.display(), e),
        Ok(d) => Ok(d),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path};

    use super::ManifestSource;
    use shipcat_definitions::Config;

    fn setup() {
        let pwd = env::current_dir().unwrap();
        let pth = fs::canonicalize(Path::new(&pwd).join("..").join("tests")).unwrap();
        std::env::set_current_dir(pth).unwrap();
    }

    #[tokio::test]
    async fn load_fake_ask() {
        setup();

        let conf = Config::read().await.unwrap();
        let region = conf.get_region("dev-uk").unwrap();

        let manifest = ManifestSource::load_manifest("fake-ask", &conf, &region)
            .await
            .unwrap();
        assert_eq!(manifest.name, "fake-ask".to_string());
    }

    #[tokio::test]
    async fn load_fake_ask_metadata() {
        setup();

        let conf = Config::read().await.unwrap();
        let region = conf.get_region("dev-uk").unwrap();

        let manifest = ManifestSource::load_metadata("fake-ask", &conf, &region)
            .await
            .unwrap();
        assert_eq!(manifest.base.name, "fake-ask".to_string());
        assert_eq!(manifest.version, Some("1.6.0".into()));
        assert_eq!(manifest.image, Some("quay.io/babylonhealth/fake-ask".into()));
    }

    #[tokio::test]
    async fn all() {
        setup();

        let conf = Config::read().await.unwrap();

        let all = ManifestSource::all(&conf).await.unwrap();

        let svc = &all[0];
        assert_eq!(svc.name, "external");

        let svc = &all[1];
        assert_eq!(svc.name, "fake-ask");

        let svc = &all[2];
        assert_eq!(svc.name, "fake-storage");

        let svc = &all[3];
        assert_eq!(svc.name, "out-of-region");
    }

    #[tokio::test]
    async fn available() {
        setup();

        let conf = Config::read().await.unwrap();
        let region = conf.get_region("dev-uk").unwrap();

        let available = ManifestSource::available(&conf, &region).await.unwrap();
        assert_eq!(available.len(), 2);

        let manifest = &available[0];
        assert_eq!(manifest.base.name, "fake-ask".to_string());

        let manifest = &available[1];
        assert_eq!(manifest.base.name, "fake-storage".to_string());
    }
}
