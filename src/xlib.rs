use anyhow::{bail, Context, Result};
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A node in the .xlib package hierarchy.
///
/// Each node may contain:
/// - `files`: source/header files to import at this level
/// - `children`: named sub-groups that become nested logical folders
#[derive(Debug, Clone)]
pub struct XLibNode {
    pub files: Vec<PathBuf>,
    pub children: BTreeMap<String, XLibNode>,
}

/// A single file entry resolved from an .xlib, ready for importing.
///
/// `logical_path` is the chain of logical folder names from root to the
/// group that contains this file, e.g. `["Drivers", "CAN"]`.
#[derive(Debug, Clone)]
pub struct ImportEntry {
    /// Absolute path to the source file on disk
    pub src_path: PathBuf,
    /// Filename only (e.g. "can.c")
    pub filename: String,
    /// Logical folder path segments (empty = root-level)
    pub logical_path: Vec<String>,
}

/// Parse a `.xlib` YAML file and return the root `XLibNode`.
///
/// All file paths inside the YAML are resolved relative to the directory
/// containing the `.xlib` file.
pub fn parse(xlib_path: &Path) -> Result<XLibNode> {
    let xlib_path = xlib_path
        .canonicalize()
        .with_context(|| format!("Cannot access .xlib file: {}", xlib_path.display()))?;
    let base_dir = xlib_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine parent of: {}", xlib_path.display()))?;

    let content = std::fs::read_to_string(&xlib_path)
        .with_context(|| format!("Failed to read .xlib file: {}", xlib_path.display()))?;

    let root_value: Value =
        serde_yaml::from_str(&content).with_context(|| format!("Malformed YAML in: {}", xlib_path.display()))?;

    let root_map = root_value
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("Expected YAML mapping at root of: {}", xlib_path.display()))?;

    parse_node(root_map, base_dir, &xlib_path)
}

/// Recursively parse a YAML mapping into an `XLibNode`.
fn parse_node(
    map: &serde_yaml::Mapping,
    base_dir: &Path,
    xlib_path: &Path,
) -> Result<XLibNode> {
    let mut files = Vec::new();
    let mut children = BTreeMap::new();

    for (key, value) in map {
        let key_str = key
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Non-string key in .xlib: {:?}", key))?;

        if key_str == "files" {
            // Parse file list
            let file_list = value
                .as_sequence()
                .ok_or_else(|| anyhow::anyhow!("`files` must be a list in: {}", xlib_path.display()))?;

            for entry in file_list {
                let rel_path_str = entry
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("`files` entries must be strings in: {}", xlib_path.display()))?;
                let resolved = base_dir.join(rel_path_str);
                if !resolved.is_file() {
                    bail!(
                        "File not found: {} (resolved to: {})",
                        rel_path_str,
                        resolved.display()
                    );
                }
                files.push(resolved);
            }
        } else {
            // Nested group — must be a mapping
            let child_map = value.as_mapping().ok_or_else(|| {
                anyhow::anyhow!(
                    "Group '{}' must be a YAML mapping in: {}",
                    key_str,
                    xlib_path.display()
                )
            })?;
            let child_node = parse_node(child_map, base_dir, xlib_path)?;
            children.insert(key_str.to_string(), child_node);
        }
    }

    Ok(XLibNode { files, children })
}

/// Flatten an `XLibNode` tree into a list of `ImportEntry` items.
///
/// Each entry carries the full logical path from root to the group it belongs to.
pub fn flatten(node: &XLibNode) -> Vec<ImportEntry> {
    let mut entries = Vec::new();
    flatten_recursive(node, &[], &mut entries);
    entries
}

fn flatten_recursive(
    node: &XLibNode,
    current_path: &[String],
    entries: &mut Vec<ImportEntry>,
) {
    for file_path in &node.files {
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        entries.push(ImportEntry {
            src_path: file_path.clone(),
            filename,
            logical_path: current_path.to_vec(),
        });
    }

    for (name, child) in &node.children {
        let mut child_path = current_path.to_vec();
        child_path.push(name.clone());
        flatten_recursive(child, &child_path, entries);
    }
}

/// Search a directory for `.xlib` files.
///
/// - If exactly one `.xlib` file is found, return its path.
/// - If none are found, return an error.
/// - If multiple are found, return an error listing them.
pub fn find_in_directory(dir: &Path) -> Result<PathBuf> {
    if !dir.is_dir() {
        bail!("Not a directory: {}", dir.display());
    }

    let xlib_files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory: {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map_or(false, |e| e == "xlib")
        })
        .collect();

    match xlib_files.len() {
        0 => bail!("No .xlib file found in: {}", dir.display()),
        1 => Ok(xlib_files.into_iter().next().unwrap()),
        n => {
            let names: Vec<String> = xlib_files
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect();
            bail!(
                "Found {} .xlib files in {} — please specify which one to use: {}",
                n,
                dir.display(),
                names.join(", ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xgraft_xlib_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_parse_simple() {
        let dir = setup_test_dir("simple");
        // Create source files
        fs::write(dir.join("main.c"), "").unwrap();
        fs::write(dir.join("main.h"), "").unwrap();

        // Create .xlib
        let xlib_content = r#"
files:
  - main.c
  - main.h
"#;
        let xlib_path = dir.join("test.xlib");
        fs::write(&xlib_path, xlib_content).unwrap();

        let node = parse(&xlib_path).unwrap();
        assert_eq!(node.files.len(), 2);
        assert!(node.children.is_empty());

        let entries = flatten(&node);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.logical_path.is_empty()));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_nested() {
        let dir = setup_test_dir("nested");
        // Create nested structure
        fs::create_dir_all(dir.join("can")).unwrap();
        fs::create_dir_all(dir.join("spi")).unwrap();
        fs::write(dir.join("can/can.c"), "").unwrap();
        fs::write(dir.join("can/can.h"), "").unwrap();
        fs::write(dir.join("spi/spi.c"), "").unwrap();
        fs::write(dir.join("spi/spi.h"), "").unwrap();

        let xlib_content = r#"
Drivers:
  CAN:
    files:
      - can/can.c
      - can/can.h
  SPI:
    files:
      - spi/spi.c
      - spi/spi.h
"#;
        let xlib_path = dir.join("drivers.xlib");
        fs::write(&xlib_path, xlib_content).unwrap();

        let node = parse(&xlib_path).unwrap();
        assert!(node.files.is_empty());
        assert_eq!(node.children.len(), 1); // "Drivers"

        let drivers = &node.children["Drivers"];
        assert_eq!(drivers.children.len(), 2); // "CAN", "SPI"
        assert_eq!(drivers.children["CAN"].files.len(), 2);
        assert_eq!(drivers.children["SPI"].files.len(), 2);

        let entries = flatten(&node);
        assert_eq!(entries.len(), 4);

        // Check logical paths
        let can_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.filename.starts_with("can"))
            .collect();
        assert!(can_entries
            .iter()
            .all(|e| e.logical_path == vec!["Drivers", "CAN"]));

        let spi_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.filename.starts_with("spi"))
            .collect();
        assert!(spi_entries
            .iter()
            .all(|e| e.logical_path == vec!["Drivers", "SPI"]));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_missing_file() {
        let dir = setup_test_dir("missing");
        let xlib_content = r#"
files:
  - nonexistent.c
"#;
        let xlib_path = dir.join("test.xlib");
        fs::write(&xlib_path, xlib_content).unwrap();

        let result = parse(&xlib_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("File not found"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_find_in_directory_single() {
        let dir = setup_test_dir("find_single");
        fs::write(dir.join("my.xlib"), "files: []").unwrap();

        let found = find_in_directory(&dir).unwrap();
        assert_eq!(found.file_name().unwrap(), "my.xlib");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_find_in_directory_multiple() {
        let dir = setup_test_dir("find_multi");
        fs::write(dir.join("a.xlib"), "files: []").unwrap();
        fs::write(dir.join("b.xlib"), "files: []").unwrap();

        let result = find_in_directory(&dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("2 .xlib files"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_find_in_directory_none() {
        let dir = setup_test_dir("find_none");
        fs::write(dir.join("some.txt"), "").unwrap();

        let result = find_in_directory(&dir);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No .xlib file found"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_mixed_root_and_nested() {
        let dir = setup_test_dir("mixed");
        fs::create_dir_all(dir.join("delay")).unwrap();
        fs::create_dir_all(dir.join("can")).unwrap();
        fs::write(dir.join("delay/delay.c"), "").unwrap();
        fs::write(dir.join("delay/delay.h"), "").unwrap();
        fs::write(dir.join("can/can.c"), "").unwrap();

        let xlib_content = r#"
files:
  - delay/delay.c
  - delay/delay.h

Drivers:
  CAN:
    files:
      - can/can.c
"#;
        let xlib_path = dir.join("mixed.xlib");
        fs::write(&xlib_path, xlib_content).unwrap();

        let node = parse(&xlib_path).unwrap();
        assert_eq!(node.files.len(), 2);
        assert_eq!(node.children.len(), 1);

        let entries = flatten(&node);
        assert_eq!(entries.len(), 3);

        // Root entries have empty logical_path
        let root_entries: Vec<_> = entries.iter().filter(|e| e.logical_path.is_empty()).collect();
        assert_eq!(root_entries.len(), 2);

        // Nested entry
        let can_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.logical_path == vec!["Drivers", "CAN"])
            .collect();
        assert_eq!(can_entries.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }
}
