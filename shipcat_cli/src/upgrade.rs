//! Interface to shipcat self-upgrade
use super::{ErrorKind, Result};
use futures::TryStreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{header, Client};
use semver::Version;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};

fn get_target() -> Result<String> {
    if !cfg!(target_arch = "x86_64") {
        bail!("shipcat is only built for 64 bit architectures");
    }
    let arch = "x86_64";

    let os_config = (cfg!(target_os = "linux"), cfg!(target_os = "macos"));
    let os = match os_config {
        (true, _) => "unknown-linux-musl",
        (_, true) => "apple-darwin",
        _ => bail!("shipcat only has assets for mac and linux"),
    };

    Ok(format!("{}-{}", arch, os))
}

struct ExeInfo {
    /// Path to current_exe
    path: PathBuf,
    /// Best guess at install prefix based on path (only for static executables)
    base: PathBuf,
}

fn identify_exe() -> Result<ExeInfo> {
    let pth = std::env::current_exe()?;
    trace!("shipcat at {}", pth.display());
    Ok(ExeInfo {
        path: pth.clone(),
        base: pth.parent().expect("current_exe has a parent dir").to_path_buf(),
    })
}

#[derive(Deserialize, Debug)]
struct ReleaseAsset {
    name: String,
    //url: String, <- not the one that makes things easy
    browser_download_url: String,
}

#[derive(Deserialize, Debug)]
struct GithubRelease {
    assets: Vec<ReleaseAsset>,
    name: String,
    tag_name: String,
}

impl GithubRelease {
    fn asset_for(&self, target: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|&x| x.name.contains(target))
    }
}

// Fetch the first page of releases from a release page
async fn fetch_latest_releases(client: &Client, url: &str) -> Result<Vec<GithubRelease>> {
    debug!("Finding latest releases: {}", url);
    let mut req = client.get(url);
    if let Ok(token) = std::env::var("SHIPCAT_AUTOUPGRADE_TOKEN") {
        req = req.bearer_auth(token);
    }
    let res = req.send().await?;

    if !res.status().is_success() {
        let status = res.status().to_owned();
        bail!("Failed to get {}: {}", url, status)
    }
    let body = res.text().await?;
    trace!("Got body: {}", body);
    let rels: Vec<GithubRelease> = serde_json::from_str(&body)?;
    Ok(rels
        .into_iter()
        .filter(|r| !r.assets.is_empty())
        .collect::<Vec<_>>())
}

/// Attempt to upgrade shipcat
pub async fn self_upgrade(ver: Option<Version>) -> Result<()> {
    debug!("self_upgrade to pin={:?}", ver);
    let running_ver = Version::parse(env!("CARGO_PKG_VERSION")).expect("could read shipcat version");

    let client = Client::builder().user_agent("rust-reqwest/shipcat").build()?;
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/releases",
        "babylonhealth", "shipcat"
    );
    let releases = fetch_latest_releases(&client, &api_url).await?;
    trace!("found releases:");
    trace!("{:#?}\n", releases);

    // If using a requested version, we have to find that
    let rel = if let Some(v) = &ver {
        releases
            .into_iter()
            .find(|r| Version::parse(&r.tag_name) == Ok(v.clone()))
    } else {
        // pick latest if upgrading opportunistically
        releases.into_iter().find(|r| !r.assets.is_empty())
    };

    let release = match rel {
        Some(r) => r,
        None => bail!("No matching shipcat release found for version: {:?}", ver),
    };
    let asset = release
        .asset_for(&get_target()?)
        .expect("Asset for valid target exists");
    debug!("selected {:?}", asset);
    if Version::parse(&release.tag_name) == Ok(running_ver.clone()) {
        // NB: Config::verify_version checks for >=, allow non-noop upgrades here
        info!("shipcat is already running {}", release.tag_name);
        return Ok(());
    }
    info!("upgrading from {} to {}", running_ver, release.tag_name);

    // using temp dir ends up causing tons of cross-link failures (18) or os error 26
    // basically; fs::rename across partition causes many problems...
    // we instead make a directory where shipcat lives, then clean that up...
    let exe = identify_exe()?;

    // Because upgrade fs::rename call can fail when moving across partitions
    // we try to avoid this by using a temp dir inside the path shipcat is found..
    let tmp_dir = exe.base.join(format!("shipcat_{}", release.tag_name));
    fs::create_dir_all(&tmp_dir).await?;
    debug!("using tmp_dir: {:?}", tmp_dir);

    let tmp_tarball_path = tmp_dir.join(&asset.name);
    debug!("tarball path: {:?}", tmp_tarball_path);

    let tmp_tarball = fs::File::create(&tmp_tarball_path).await?;
    debug!("tmp tarball: {:?}", tmp_tarball);

    let dl_url = &asset.browser_download_url;
    download_tarball(&client, &dl_url, tmp_tarball).await?;
    let bin_path = std::path::PathBuf::from("bin/shipcat");
    extract_tarball(&tmp_dir, tmp_tarball_path, &bin_path)?;

    debug!("Replacing {:?}", exe.path);
    let swap = tmp_dir.join("replacement");
    let src = tmp_dir.join("bin").join("shipcat");
    let dest = exe.path;

    debug!("Backing up {} to {}", dest.display(), swap.display());
    fs::rename(&dest, &swap).await?;

    debug!("Swapping in new version of shipcat {:?} -> {:?}", src, dest);
    if let Err(e) = fs::rename(&src, &dest).await {
        warn!("rollback rename: {:?} -> {:?} due to {}", swap, dest, e);
        fs::rename(&swap, &dest).await?; // fallback on error TODO: ?
    }
    fs::remove_dir_all(&tmp_dir).await?;
    Ok(())
}

/// Download the file behind the given `url` into the specified `dest`.
///
/// Presents a progressbar via indicatif when content-length is returned
async fn download_tarball(client: &Client, url: &str, mut dest: File) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    debug!("Downloading tarball: {}", url);
    let mut req = client.get(url);

    req = req.header(header::ACCEPT, "application/octet-stream");
    if let Ok(token) = std::env::var("SHIPCAT_AUTOUPGRADE_TOKEN") {
        req = req.bearer_auth(token);
    }
    let res = req.send().await?;
    let size = res.content_length().unwrap_or(0); // for progress-bar length
    if !res.status().is_success() {
        let status = res.status().to_owned();
        bail!("Failed to download {}: {}", url, status)
    }

    // progress-bar
    let mut downloaded = 0;
    let mut pbar = if size > 0 {
        let pb = ProgressBar::new(size);
        pb.set_style(
            ProgressStyle::default_bar().template("{bar:40.green/black} {bytes}/{total_bytes} ({eta}) {msg}"),
        );
        pb.set_message("downloading");
        Some(pb)
    } else {
        None
    };

    // chunked writing
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.try_next().await? {
        dest.write_all(&chunk).await?;
        let n = chunk.len();
        if let Some(ref mut pb) = pbar {
            downloaded = std::cmp::min(downloaded + n as u64, size);
            pb.set_position(downloaded);
        }
    }
    if let Some(ref mut pb) = pbar {
        pb.finish_and_clear();
    }
    Ok(())
}

use std::io::{Read, Seek, SeekFrom};
// A generic progress-bar reader to slap between readers
pub struct ProgressReader<R: Read + Seek> {
    rdr: R,
    pb: ProgressBar,
}
impl<R: Read + Seek> ProgressReader<R> {
    pub fn new(mut rdr: R, msg: &str) -> std::io::Result<ProgressReader<R>> {
        let len = rdr.seek(SeekFrom::End(0))?;
        rdr.seek(SeekFrom::Start(0))?;
        let pb = ProgressBar::new(len);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{bar:40.yellow/black} {bytes}/{total_bytes} ({eta}) {msg}"),
        );
        pb.set_message(msg);
        Ok(ProgressReader { rdr, pb })
    }
}
impl<R: Read + Seek> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let rv = self.rdr.read(buf)?;
        self.pb.inc(rv as u64);
        Ok(rv)
    }
}

/// Extract a tarball from `src` into a specified `extract_path`
///
/// Updates with a yellow progress-bar while extraction is happening.
fn extract_tarball<T: AsRef<Path>>(extract_path: &Path, src: T, file_to_extract: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::fs;
    use tar::Archive; // no async tar/gz stuff afaikt

    let data = fs::File::open(src.as_ref())?;
    let progdata = ProgressReader::new(data, "extracting")?;
    let decompressed = GzDecoder::new(progdata);
    let mut archive = Archive::new(decompressed); // Archive reads decoded

    let mut entry = archive
        .entries()?
        .filter_map(|e| e.ok())
        .find(|e| e.path().ok().filter(|p| p == file_to_extract).is_some())
        .ok_or_else(|| {
            ErrorKind::SelfUpgradeError(format!("Could not find path in archive: {:?}", file_to_extract))
        })?;
    entry.unpack_in(&extract_path)?;
    Ok(())
}
