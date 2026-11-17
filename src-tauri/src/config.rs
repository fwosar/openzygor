use crate::model::{Game, Region};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Installation {
    pub id: String,
    pub game: Game,
    pub path: PathBuf,
}
impl Installation {
    pub fn new(game: Game, path: PathBuf) -> Self {
        use md5::{Digest, Md5};
        let id = format!(
            "{}-{:x}",
            game.id(),
            Md5::digest(path.to_string_lossy().as_bytes())
        );
        Self { id, game, path }
    }
    pub fn addons(&self) -> PathBuf {
        self.path.join("Interface").join("AddOns")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub username: String,
    pub remember: bool,
    pub region: Region,
    pub installations: Vec<Installation>,
    #[serde(alias = "check_automatically")]
    pub update_automatically: bool,
}

pub fn load(path: &Path) -> Result<Config> {
    match fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).context("The OpenZygor settings file is damaged. Restore it from a backup or rename it to start fresh."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(error).context("Could not read OpenZygor settings"),
    }
}

pub fn save(path: &Path, config: &Config) -> Result<()> {
    let parent = path.parent().context("Settings path has no parent")?;
    fs::create_dir_all(parent).context("Could not create the settings directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(config)?)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .context("Could not save OpenZygor settings")?;
    Ok(())
}
