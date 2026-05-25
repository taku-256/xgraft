// Author: taku-256

mod project;
mod xlib;
mod xml;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::io::{self, Write};
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "xgraft",
    about = "Import source/header files into an MPLAB X IDE project",
    version,
    author = "taku-256"
)]
struct Cli {
    /// Path to the MPLAB X project (.X directory or its parent)
    project_path: String,

    /// Files to import (.c, .h, .cpp, .hpp, .xlib, or directories containing .xlib)
    #[arg(required_unless_present = "libs")]
    files: Vec<String>,

    /// Library files to import (.xlib, or directories containing .xlib, or source/header files)
    #[arg(short = 'l', long = "library", value_name = "FILE")]
    libs: Vec<String>,

    /// Force overwrite without prompting
    #[arg(long = "force", short = 'f')]
    force: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // 1. Resolve project
    let proj = project::resolve(&cli.project_path)?;
    eprintln!(
        "Project: {}",
        proj.x_dir.file_name().unwrap().to_string_lossy()
    );

    // 2. Separate inputs into direct files and .xlib packages
    let mut direct_items: Vec<xml::ImportItem> = Vec::new();
    let mut xlib_items: Vec<xml::XLibImportItem> = Vec::new();

    let all_files = cli.libs.iter().chain(cli.files.iter());
    for file_str in all_files {
        let src_path = Path::new(file_str);

        if src_path.is_dir() {
            // Directory: find a single .xlib inside
            let xlib_path = xlib::find_in_directory(src_path)?;
            eprintln!("  found: {}", xlib_path.display());
            process_xlib(&xlib_path, &proj, cli.force, &mut xlib_items)?;
            continue;
        }

        let ext = src_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if ext == "xlib" {
            // .xlib file
            if !src_path.is_file() {
                bail!("File not found: {}", file_str);
            }
            process_xlib(src_path, &proj, cli.force, &mut xlib_items)?;
        } else {
            // Direct source/header file (existing behavior)
            process_direct_file(src_path, file_str, &proj, cli.force, &mut direct_items)?;
        }
    }

    // 3. Update configurations.xml
    let mut total_added = 0;

    if !direct_items.is_empty() {
        let added = xml::update_configurations(&proj.configurations_xml, &direct_items)?;
        total_added += added.len();
    }

    if !xlib_items.is_empty() {
        let added = xml::update_configurations_xlib(&proj.configurations_xml, &xlib_items)?;
        total_added += added.len();
    }

    if total_added == 0 && direct_items.is_empty() && xlib_items.is_empty() {
        eprintln!("No files to import.");
    } else {
        eprintln!("\nDone. {} file(s) imported.", total_added);
    }

    Ok(())
}

/// Process a direct source/header file: validate, copy, and add to import list.
fn process_direct_file(
    src_path: &Path,
    file_str: &str,
    proj: &project::Project,
    force: bool,
    items: &mut Vec<xml::ImportItem>,
) -> Result<()> {
    if !src_path.is_file() {
        bail!("File not found: {}", file_str);
    }

    let filename = src_path
        .file_name()
        .and_then(|n| n.to_str())
        .context(format!("Invalid filename: {}", file_str))?
        .to_string();

    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let kind = xml::classify_extension(ext).ok_or_else(|| {
        anyhow::anyhow!(
            "Unsupported file type: {} (expected .c/.h/.cpp/.hpp)",
            filename
        )
    })?;

    let dest_path = proj.x_dir.join(&filename);
    if dest_path.exists() {
        if force {
            eprintln!("  overwrite (--force): {}", filename);
        } else if !prompt_overwrite(&filename)? {
            eprintln!("  skip: {}", filename);
            return Ok(());
        }
    }

    std::fs::copy(src_path, &dest_path)
        .with_context(|| format!("Failed to copy {} -> {}", file_str, dest_path.display()))?;
    eprintln!("  copied: {}", filename);

    items.push(xml::ImportItem { filename, kind });
    Ok(())
}

/// Process an .xlib package file: parse, copy files, and add to import list.
fn process_xlib(
    xlib_path: &Path,
    proj: &project::Project,
    force: bool,
    items: &mut Vec<xml::XLibImportItem>,
) -> Result<()> {
    eprintln!(
        "  loading: {}",
        xlib_path.file_name().unwrap().to_string_lossy()
    );

    let node = xlib::parse(xlib_path)?;
    let entries = xlib::flatten(&node);

    if entries.is_empty() {
        eprintln!("  (no files in .xlib)");
        return Ok(());
    }

    for entry in &entries {
        let ext = entry
            .src_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let kind = xml::classify_extension(ext).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported file type in .xlib: {} (expected .c/.h/.cpp/.hpp)",
                entry.filename
            )
        })?;

        // Copy file into project directory
        let dest_path = proj.x_dir.join(&entry.filename);
        if dest_path.exists() {
            if force {
                eprintln!("  overwrite (--force): {}", entry.filename);
            } else if !prompt_overwrite(&entry.filename)? {
                eprintln!("  skip: {}", entry.filename);
                continue;
            }
        }

        std::fs::copy(&entry.src_path, &dest_path).with_context(|| {
            format!(
                "Failed to copy {} -> {}",
                entry.src_path.display(),
                dest_path.display()
            )
        })?;
        eprintln!("  copied: {}", entry.filename);

        items.push(xml::XLibImportItem {
            filename: entry.filename.clone(),
            kind,
            logical_path: entry.logical_path.clone(),
        });
    }

    Ok(())
}

/// Prompt the user with Y/n for overwriting a file
fn prompt_overwrite(filename: &str) -> Result<bool> {
    eprint!("  {} already exists. Overwrite? [Y/n] ", filename);
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    Ok(trimmed.is_empty() || trimmed == "y" || trimmed == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_positional_only() {
        let args = vec!["xgraft", "my_project", "file.c", "file.h"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.project_path, "my_project");
        assert_eq!(cli.files, vec!["file.c".to_string(), "file.h".to_string()]);
        assert!(cli.libs.is_empty());
        assert!(!cli.force);
    }

    #[test]
    fn test_cli_lib_only() {
        let args = vec!["xgraft", "-l", "mylib.xlib", "my_project"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.project_path, "my_project");
        assert!(cli.files.is_empty());
        assert_eq!(cli.libs, vec!["mylib.xlib".to_string()]);
        assert!(!cli.force);
    }

    #[test]
    fn test_cli_lib_and_positional() {
        let args = vec!["xgraft", "-l", "mylib.xlib", "my_project", "extra.c"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.project_path, "my_project");
        assert_eq!(cli.files, vec!["extra.c".to_string()]);
        assert_eq!(cli.libs, vec!["mylib.xlib".to_string()]);
    }

    #[test]
    fn test_cli_multiple_libs() {
        let args = vec!["xgraft", "-l", "lib1.xlib", "--library", "lib2.xlib", "my_project"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.project_path, "my_project");
        assert!(cli.files.is_empty());
        assert_eq!(cli.libs, vec!["lib1.xlib".to_string(), "lib2.xlib".to_string()]);
    }

    #[test]
    fn test_cli_missing_files_and_libs() {
        let args = vec!["xgraft", "my_project"];
        let res = Cli::try_parse_from(args);
        assert!(res.is_err());
    }
}

