//! # The codex
//!
//! The codex is the set of genes that the player has taken in any run. A run
//! puts each gene it took (by engulf, virus or gate cell) into the codex when it
//! ends, win or loss. On the Prepare screen the player spends points on codex
//! genes to start the next run with them (see `prepare`).
//!
//! The codex is a JSON file, `codex.json`, in the save directory:
//! `METABOLISTIC_SAVE_DIR` if it is set, else the user data directory
//! (`~/Library/Application Support/metabolistic3d` on macOS,
//! `$XDG_DATA_HOME/metabolistic3d` or `~/.local/share/metabolistic3d` on Linux,
//! `%APPDATA%\metabolistic3d` on Windows). A missing or broken file gives an
//! empty codex and a warning. The headless app keeps the codex in memory
//! (`CodexStore { path: None }`) so that tests never touch the real file.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::blocks::genome::BlockKind;

/// Name of the codex file in the save directory.
pub const CODEX_FILE: &str = "codex.json";
/// Environment variable that sets the save directory.
pub const SAVE_DIR_VAR: &str = "METABOLISTIC_SAVE_DIR";

/// The genes that the player has taken in earlier runs.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Codex {
    pub genes: BTreeSet<BlockKind>,
}

impl Codex {
    /// Adds the genes. Returns the ones that were not in the codex, in order.
    pub fn bank(&mut self, genes: &[BlockKind]) -> Vec<BlockKind> {
        genes.iter().copied().filter(|g| self.genes.insert(*g)).collect()
    }
}

/// Where the codex is saved. `None`: only in memory.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexStore {
    pub path: Option<PathBuf>,
}

/// The default save directory for a platform, from the environment values
/// (pure, for tests). `None` if no home is known.
pub fn default_save_dir(
    os: &str,
    home: Option<&Path>,
    xdg_data_home: Option<&Path>,
    appdata: Option<&Path>,
) -> Option<PathBuf> {
    let app = "metabolistic3d";
    match os {
        "macos" => home.map(|h| h.join("Library").join("Application Support").join(app)),
        "windows" => appdata.map(|a| a.join(app)),
        _ => xdg_data_home
            .map(|x| x.join(app))
            .or_else(|| home.map(|h| h.join(".local").join("share").join(app))),
    }
}

/// The save directory of this machine: `METABOLISTIC_SAVE_DIR`, else the user
/// data directory.
pub fn save_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(SAVE_DIR_VAR).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    default_save_dir(
        std::env::consts::OS,
        var("HOME").as_deref(),
        var("XDG_DATA_HOME").as_deref(),
        var("APPDATA").as_deref(),
    )
}

/// The codex file in a save directory.
pub fn codex_path(dir: &Path) -> PathBuf {
    dir.join(CODEX_FILE)
}

/// Loads the codex. A missing file is an empty codex. A broken file is an empty
/// codex and a warning.
pub fn load(path: &Path) -> Codex {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
            warn!("codex: {} is broken ({err}); starting with an empty codex", path.display());
            Codex::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Codex::default(),
        Err(err) => {
            warn!("codex: cannot read {}: {err}; starting with an empty codex", path.display());
            Codex::default()
        }
    }
}

/// Saves the codex, and makes the directory if needed.
pub fn save(codex: &Codex, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(codex).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Adds the genes of a run to the codex and saves it if there is a path.
/// Returns the genes that are new.
pub fn bank_and_save(codex: &mut Codex, store: &CodexStore, genes: &[BlockKind]) -> Vec<BlockKind> {
    let new = codex.bank(genes);
    if let Some(path) = &store.path
        && let Err(err) = save(codex, path)
    {
        warn!("codex: cannot save {}: {err}", path.display());
    }
    new
}

/// Loads the codex from the save directory at start up. `in_memory` keeps it
/// out of the file system (headless apps).
pub struct CodexPlugin {
    pub in_memory: bool,
}

impl Plugin for CodexPlugin {
    fn build(&self, app: &mut App) {
        let path = if self.in_memory { None } else { save_dir().map(|d| codex_path(&d)) };
        let codex = path.as_deref().map(load).unwrap_or_default();
        app.insert_resource(codex).insert_resource(CodexStore { path });
    }
}

#[cfg(test)]
pub(crate) mod test_dir {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A new empty directory in the system temp dir. Removed on drop.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("metabolistic3d-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_dir::TempDir;
    use super::*;

    #[test]
    fn bank_returns_only_new_genes_in_order() {
        let mut codex = Codex::default();
        assert_eq!(codex.bank(&[BlockKind::Fermentation, BlockKind::FatStorage]), vec![BlockKind::Fermentation, BlockKind::FatStorage]);
        assert_eq!(
            codex.bank(&[BlockKind::FatStorage, BlockKind::LightCapture, BlockKind::LightCapture]),
            vec![BlockKind::LightCapture]
        );
        assert_eq!(codex.genes.len(), 3);
    }

    #[test]
    fn the_codex_survives_a_save_and_load_in_a_new_directory() {
        let dir = TempDir::new();
        let path = codex_path(&dir.0.join("nested").join("save"));
        let mut codex = Codex::default();
        codex.bank(&[BlockKind::SecondaryMetabolites, BlockKind::VesicleExport]);
        save(&codex, &path).unwrap();
        assert_eq!(load(&path), codex);
    }

    #[test]
    fn a_missing_file_gives_an_empty_codex() {
        let dir = TempDir::new();
        assert_eq!(load(&codex_path(&dir.0)), Codex::default());
    }

    #[test]
    fn a_broken_file_gives_an_empty_codex_and_no_panic() {
        let dir = TempDir::new();
        let path = codex_path(&dir.0);
        for text in ["not json", "{\"genes\": [\"NoSuchGene\"]}", "", "[1,2"] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(load(&path), Codex::default(), "{text:?}");
        }
    }

    #[test]
    fn bank_and_save_writes_only_to_its_own_path() {
        let dir = TempDir::new();
        let store = CodexStore { path: Some(codex_path(&dir.0)) };
        let mut codex = Codex::default();
        let new = bank_and_save(&mut codex, &store, &[BlockKind::Fermentation]);
        assert_eq!(new, vec![BlockKind::Fermentation]);
        assert_eq!(load(&codex_path(&dir.0)).genes.len(), 1);
        // No path: nothing is written.
        let mut other = Codex::default();
        bank_and_save(&mut other, &CodexStore::default(), &[BlockKind::FatStorage]);
        assert!(other.genes.contains(&BlockKind::FatStorage));
    }

    #[test]
    fn default_save_dir_follows_the_platform() {
        let home = Path::new("/home/u");
        assert_eq!(
            default_save_dir("macos", Some(home), None, None),
            Some(PathBuf::from("/home/u/Library/Application Support/metabolistic3d"))
        );
        assert_eq!(
            default_save_dir("linux", Some(home), None, None),
            Some(PathBuf::from("/home/u/.local/share/metabolistic3d"))
        );
        assert_eq!(
            default_save_dir("linux", Some(home), Some(Path::new("/x")), None),
            Some(PathBuf::from("/x/metabolistic3d"))
        );
        assert_eq!(default_save_dir("macos", None, None, None), None);
    }
}
