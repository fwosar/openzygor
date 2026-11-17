use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const HISTORY_LIMIT: usize = 100;
const MESSAGE_LIMIT: usize = 4096;
const FILE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub timestamp_ms: u64,
    pub message: String,
    pub failed: bool,
}

pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("Could not read activity history"),
    };
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= FILE_LIMIT,
        "Activity history is too large to read"
    );
    let mut entries: Vec<Entry> =
        serde_json::from_slice(&bytes).context("Activity history is damaged")?;
    entries.truncate(HISTORY_LIMIT);
    for entry in &mut entries {
        ensure!(
            entry.timestamp_ms <= 8_640_000_000_000_000,
            "Activity history contains an invalid date"
        );
        entry.message = entry.message.chars().take(MESSAGE_LIMIT).collect();
    }
    Ok(entries)
}

pub fn append(path: &Path, message: &str, failed: bool) -> Result<Vec<Entry>> {
    with_lock(path, || {
        let mut entries = load(path)?;
        entries.insert(
            0,
            Entry {
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)?
                    .as_millis()
                    .try_into()?,
                message: message.chars().take(MESSAGE_LIMIT).collect(),
                failed,
            },
        );
        entries.truncate(HISTORY_LIMIT);
        save(path, &entries)?;
        Ok(entries)
    })
}

pub fn clear(path: &Path) -> Result<Vec<Entry>> {
    with_lock(path, || {
        // Clearing also lets the user discard a damaged history file.
        save(path, &[])?;
        Ok(Vec::new())
    })
}

fn with_lock<T>(path: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let parent = path.parent().context("Activity path has no parent")?;
    fs::create_dir_all(parent).context("Could not create activity directory")?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path.with_extension("lock"))?;
    fs2::FileExt::lock_exclusive(&lock).context("Could not lock activity history")?;
    // The file lock serializes read-modify-write across application instances.
    action()
}

fn save(path: &Path, entries: &[Entry]) -> Result<()> {
    let parent = path.parent().context("Activity path has no parent")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(entries)?)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .context("Could not save activity history")?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
