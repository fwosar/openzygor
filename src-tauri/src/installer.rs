use crate::{config::Installation, discovery};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use md5::{Digest, Md5};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

const MAX_FILES: usize = 100_000;
const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;
const MAX_FILE: u64 = 128 * 1024 * 1024;

pub fn product_name(name: &str) -> Result<()> {
    ensure!(
        name.starts_with("Zygor")
            && name.len() <= 100
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "Zygor returned an unsafe addon directory name"
    );
    Ok(())
}

pub fn version(addon: &Path, product: &str) -> Option<String> {
    product_name(product).ok()?;
    let toc = fs::read_to_string(addon.join(format!("{product}.toc"))).ok()?;
    let major = toc
        .lines()
        .find_map(|line| line.strip_prefix("## Version:"))?
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>();
    if major.is_empty() {
        return None;
    }
    let lua = fs::read_to_string(addon.join("Ver.lua")).ok()?;
    let revision: String = lua
        .split("Revision:")
        .nth(1)?
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if revision.is_empty() {
        return None;
    }
    // The official addon stores major.minor in the TOC and the revision in Ver.lua.
    Some(format!("{major}.{revision}"))
}

fn no_symlink(path: &Path) -> Result<()> {
    // Reject symlinks throughout the path, including Interface and AddOns.
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "Refusing to write through symlink: {}",
                ancestor.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e).context("Could not inspect installation path"),
        }
    }
    Ok(())
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    ensure!(!cancel.is_cancelled(), "Download cancelled");
    Ok(())
}

fn file_manifest(
    source: &Path,
    product: &str,
    copy_to: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut manifest = String::new();
    let mut total = 0u64;
    no_symlink(source)?;
    if !source.exists() {
        return Ok(manifest);
    }
    ensure!(source.is_dir(), "The addon path is not a directory");
    for (index, entry) in walkdir::WalkDir::new(source)
        .sort_by_file_name()
        .follow_links(false)
        .into_iter()
        .enumerate()
    {
        check_cancel(cancel)?;
        ensure!(index < MAX_FILES, "Too many files in the addon directory");
        let entry = entry.context("Could not read the addon directory")?;
        ensure!(
            !entry.file_type().is_symlink(),
            "The addon contains a symlink; move it outside the addon before updating"
        );
        let relative = entry.path().strip_prefix(source)?;
        if entry.file_type().is_dir() {
            if let Some(target) = copy_to {
                fs::create_dir_all(target.join(relative))?;
            }
            continue;
        }
        ensure!(
            entry.file_type().is_file(),
            "The addon contains a special file"
        );
        let name = relative
            .to_str()
            .context("An addon filename is not valid UTF-8")?;
        let name = name.replace(std::path::MAIN_SEPARATOR, "/");
        ensure!(
            !name.contains(['\r', '\n', '\\']) && !name.contains("##"),
            "The addon contains a filename the update protocol cannot represent"
        );
        let mut input = File::open(entry.path())?;
        let mut output = copy_to
            .map(|target| File::create(target.join(relative)))
            .transpose()?;
        let mut hash = Md5::new();
        let mut size = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            check_cancel(cancel)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            total += count as u64;
            ensure!(
                size <= MAX_FILE && total <= MAX_UNPACKED,
                "The existing addon exceeds the size limit"
            );
            hash.update(&buffer[..count]);
            if let Some(output) = output.as_mut() {
                output.write_all(&buffer[..count])?;
            }
        }
        if let Some(output) = output {
            output.sync_all()?;
        }
        manifest.push_str(&format!("F {product}/{name}##{:x}\r\n", hash.finalize()));
    }
    Ok(manifest)
}

/// The file lock spans hashing, the network request, staging, and commit.
pub struct PreparedInstall {
    _lock: File,
    stage: tempfile::TempDir,
    work: PathBuf,
    destination: PathBuf,
    product: String,
    pub manifest: String,
    pub installed_version: Option<String>,
}

impl PreparedInstall {
    pub fn prepare(
        installation: &Installation,
        product: &str,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        product_name(product)?;
        let checked = discovery::validate(installation.game, &installation.path)?;
        let addons = checked.addons();
        no_symlink(&addons)?;
        fs::create_dir_all(&addons)
            .context("Could not create Interface/AddOns; check folder permissions")?;
        let destination = addons.join(product);
        no_symlink(&destination)?;
        let work = addons.join(format!(".openzygor-{product}"));
        no_symlink(&work)?;
        fs::create_dir_all(&work)?;
        let lock_path = work.join("lock");
        no_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .context("Another OpenZygor process is updating this installation")?;
        let previous = work.join("previous");
        no_symlink(&previous)?;
        // A crash between the two renames is recovered before calculating checksums.
        if !destination.exists() && previous.exists() {
            fs::rename(&previous, &destination)
                .context("Could not restore an interrupted installation")?;
        }
        let stage = tempfile::Builder::new()
            .prefix("stage-")
            .tempdir_in(&work)?;
        let staged_addon = stage.path().join(product);
        fs::create_dir(&staged_addon)?;
        let installed_version = version(&destination, product);
        let manifest = file_manifest(&destination, product, Some(&staged_addon), cancel)?;
        Ok(Self {
            _lock: lock,
            stage,
            work,
            destination,
            product: product.to_owned(),
            manifest,
            installed_version,
        })
    }

    pub fn apply(
        self,
        archive: &[u8],
        license: &str,
        expected: Option<&str>,
        cancel: &CancellationToken,
        progress: impl Fn(u64, Option<u64>),
    ) -> Result<String> {
        check_cancel(cancel)?;
        let mut zip =
            zip::ZipArchive::new(Cursor::new(archive)).context("The guide archive is damaged")?;
        ensure!(
            zip.len() <= MAX_FILES,
            "The guide archive contains too many files"
        );
        let mut total = 0u64;
        let mut actual_total = 0u64;
        let mut seen = HashSet::new();
        let mut deletes = Vec::new();
        let count = zip.len() as u64;
        for index in 0..zip.len() {
            check_cancel(cancel)?;
            let mut entry = zip
                .by_index(index)
                .context("Could not read the guide archive")?;
            let name = entry.name().replace('\\', "/");
            let is_metadata = name
                .rsplit('/')
                .next()
                .is_some_and(|n| n.starts_with(".zygor-meta"));
            let relative = safe_path(&name, &self.product, is_metadata)?;
            ensure!(
                seen.insert(relative.clone()),
                "The guide archive has duplicate paths"
            );
            let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
            ensure!(
                matches!(mode, 0 | 0o100000 | 0o040000),
                "The guide archive contains a link or special file"
            );
            total = total
                .checked_add(entry.size())
                .context("The guide archive is too large")?;
            ensure!(
                entry.size() <= MAX_FILE && total <= MAX_UNPACKED,
                "The guide archive exceeds the extraction size limit"
            );
            let target = self.stage.path().join(&relative);
            if entry.is_dir() {
                fs::create_dir_all(target)?;
            } else if is_metadata {
                ensure!(
                    entry.size() <= 8 * 1024 * 1024,
                    "Update metadata exceeds the size limit"
                );
                let mut data = Vec::new();
                entry.take(8 * 1024 * 1024 + 1).read_to_end(&mut data)?;
                ensure!(
                    data.len() <= 8 * 1024 * 1024,
                    "Update metadata exceeds the size limit"
                );
                #[derive(serde::Deserialize)]
                struct Meta {
                    #[serde(default, rename = "DELETE")]
                    delete: Option<Vec<String>>,
                }
                let metadata: Meta =
                    serde_json::from_slice(&data).context("Invalid update deletion metadata")?;
                deletes.extend(metadata.delete.unwrap_or_default());
            } else {
                fs::create_dir_all(target.parent().context("Invalid archive path")?)?;
                let mut file = File::create(target)?;
                let mut buffer = [0u8; 65536];
                let mut actual_size = 0u64;
                loop {
                    check_cancel(cancel)?;
                    let n = entry.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    actual_size += n as u64;
                    actual_total += n as u64;
                    ensure!(
                        actual_size <= MAX_FILE && actual_total <= MAX_UNPACKED,
                        "The guide archive exceeds the extraction size limit"
                    );
                    file.write_all(&buffer[..n])?;
                }
                file.sync_all()?;
            }
            progress((index + 1) as u64, Some(count));
        }
        for name in deletes {
            check_cancel(cancel)?;
            let relative = safe_path(&name.replace('\\', "/"), &self.product, false)?;
            ensure!(
                relative.components().count() > 1,
                "Update metadata cannot delete the addon directory"
            );
            let target = self.stage.path().join(relative);
            if target.exists() {
                ensure!(
                    target.is_file(),
                    "Update metadata attempted to remove a directory"
                );
                fs::remove_file(target)?;
            }
        }
        let staged = self.stage.path().join(&self.product);
        let actual = version(&staged, &self.product)
            .context("The downloaded addon is missing its version files")?;
        if let Some(expected) = expected {
            ensure!(
                actual == expected,
                "Downloaded version {actual} differs from the advertised version {expected}. Check for updates and try again."
            );
        }
        fs::write(staged.join("Licence.lua"), license)?;
        File::open(staged.join("Licence.lua"))?.sync_all()?;
        ensure!(
            file_manifest(&self.destination, &self.product, None, cancel)? == self.manifest,
            "Your addon files changed during the download. Your edits were preserved; retry the update."
        );
        check_cancel(cancel)?;
        // Cancellation is intentionally deferred during this short commit section.
        no_symlink(&self.destination)?;
        let previous = self.work.join("previous");
        no_symlink(&previous)?;
        if previous.exists() {
            fs::remove_dir_all(&previous).context("Could not replace the previous backup")?;
        }
        if self.destination.exists() {
            fs::rename(&self.destination, &previous)
                .context("Could not back up the installed addon")?;
        }
        if let Err(error) = fs::rename(&staged, &self.destination) {
            if previous.exists() {
                fs::rename(&previous, &self.destination).context("Install failed and rollback could not complete; your addon is in the .openzygor backup directory")?;
            }
            return Err(error).context(
                "Could not activate the new guides; the previous installation was restored",
            );
        }
        File::open(self.destination.parent().context("Missing AddOns folder")?)?.sync_all()?;
        Ok(actual)
    }
}

fn safe_path(name: &str, product: &str, allow_metadata: bool) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains(['\0', ':', '\r', '\n']) && !name.starts_with('/'),
        "Unsafe path in guide archive"
    );
    ensure!(
        !name.split('/').any(|s| s == "." || s == ".."),
        "Path traversal in guide archive"
    );
    let path = Path::new(name);
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "Unsafe path in guide archive"
    );
    let first = path.components().next().context("Empty archive path")?;
    if allow_metadata && name.starts_with(".zygor-meta") && !name.contains('/') {
        return Ok(path.to_owned());
    }
    ensure!(
        first.as_os_str() == product,
        "Guide archive attempted to write outside its addon directory"
    );
    Ok(path.to_owned())
}

pub fn uninstall(installation: &Installation, product: &str) -> Result<()> {
    let prepared = PreparedInstall::prepare(installation, product, &CancellationToken::new())?;
    if !prepared.destination.exists() {
        bail!("This addon is not installed");
    }
    // Retain the removed addon outside AddOns' loadable directories for manual recovery.
    let removed = prepared.work.join("uninstalled");
    no_symlink(&removed)?;
    ensure!(
        !removed.exists(),
        "A previous uninstall backup exists. Move or remove it before uninstalling again."
    );
    fs::rename(&prepared.destination, removed).context("Could not uninstall the addon")?;
    // A previous update backup must not be mistaken for an interrupted commit on the next install.
    let previous = prepared.work.join("previous");
    if previous.exists() {
        fs::remove_dir_all(previous)?;
    }
    Ok(())
}
