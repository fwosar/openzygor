use crate::{config::Installation, model::Game};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub fn validate(game: Game, path: &Path) -> Result<Installation> {
    let mut path = path.to_path_buf();
    if path.file_name().is_some_and(|n| n == "AddOns") {
        path = path
            .parent()
            .and_then(Path::parent)
            .context("Select a WoW edition folder")?
            .to_path_buf();
    }
    if path.join(game.folder()).is_dir() {
        path = path.join(game.folder());
    }
    let path = path
        .canonicalize()
        .context("The selected game folder does not exist")?;
    ensure!(
        path.join(game.exe()).is_file(),
        "Choose the {} folder containing {}",
        game.folder(),
        game.exe()
    );
    // Retail and Classic executables overlap across editions; reject a known mismatched folder.
    for other in Game::ALL {
        ensure!(
            other == game || path.file_name().is_none_or(|n| n != other.folder()),
            "This folder belongs to {}",
            other.title()
        );
    }
    Ok(Installation::new(game, path))
}

pub fn discover(home: &Path) -> Vec<Installation> {
    let mut roots = BTreeSet::new();
    roots.insert(home.join(".wine/drive_c"));
    for bottles in [
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
        home.join(".local/share/bottles/bottles"),
    ] {
        for entry in directories(&bottles) {
            roots.insert(entry.join("drive_c"));
        }
    }
    // Bounded scans cover Lutris/Heroic and Steam/Proton without traversing the whole home.
    for base in [
        home.join("Games"),
        home.join(".local/share/Steam/steamapps/compatdata"),
        home.join(".steam/steam/steamapps/compatdata"),
    ] {
        for entry in walkdir::WalkDir::new(base)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_dir() && entry.file_name() == "drive_c" {
                roots.insert(entry.path().to_path_buf());
            }
        }
    }
    let mut game_roots = vec![home.join("Games/World of Warcraft")];
    for root in roots {
        for program_files in ["Program Files (x86)", "Program Files", "Games", ""] {
            game_roots.push(root.join(program_files).join("World of Warcraft"));
        }
    }
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for root in game_roots {
        for game in Game::ALL {
            if let Ok(installation) = validate(game, &root.join(game.folder()))
                && seen.insert(installation.id.clone())
            {
                found.push(installation);
            }
        }
    }
    found
}

fn directories(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect()
}
