use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Resolved MPLAB X project paths
pub struct Project {
    /// Path to the .X project directory
    pub x_dir: PathBuf,
    /// Path to nbproject/configurations.xml
    pub configurations_xml: PathBuf,
}

/// Resolve the MPLAB X project from the given path.
///
/// The path can be:
/// - A `.X` directory directly
/// - A parent directory containing exactly one `.X` subdirectory
pub fn resolve(path: &str) -> Result<Project> {
    let path = Path::new(path)
        .canonicalize()
        .with_context(|| format!("Cannot access path: {}", path))?;

    let x_dir = if is_x_dir(&path) {
        path
    } else {
        find_single_x_dir(&path)?
    };

    let configurations_xml = x_dir.join("nbproject").join("configurations.xml");
    if !configurations_xml.exists() {
        bail!(
            "configurations.xml not found at: {}",
            configurations_xml.display()
        );
    }

    Ok(Project {
        x_dir,
        configurations_xml,
    })
}

/// Check if a directory name ends with `.X`
fn is_x_dir(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .map_or(false, |n| n.ends_with(".X"))
}

/// Find exactly one `.X` directory inside the given parent directory
fn find_single_x_dir(parent: &Path) -> Result<PathBuf> {
    if !parent.is_dir() {
        bail!("Not a directory: {}", parent.display());
    }

    let x_dirs: Vec<PathBuf> = std::fs::read_dir(parent)
        .with_context(|| format!("Cannot read directory: {}", parent.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| is_x_dir(p))
        .collect();

    match x_dirs.len() {
        0 => bail!("No .X project directory found in: {}", parent.display()),
        1 => Ok(x_dirs.into_iter().next().unwrap()),
        n => bail!(
            "Found {} .X directories in {}. Please specify which one to use.",
            n,
            parent.display()
        ),
    }
}
