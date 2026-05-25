use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Logical folder targets in configurations.xml
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    HeaderFiles,
    SourceFiles,
}



/// Classify a file extension into a folder kind
pub fn classify_extension(ext: &str) -> Option<FolderKind> {
    match ext {
        "h" | "hpp" => Some(FolderKind::HeaderFiles),
        "c" | "cpp" => Some(FolderKind::SourceFiles),
        _ => None,
    }
}

/// Insertion point for new itemPath entries
struct InsertionPoint {
    /// Byte position to insert at (right after the `\n` preceding `</logicalFolder>`)
    pos: usize,
    /// Indentation string to use for new `<itemPath>` elements
    item_indent: String,
}

/// Information gathered from a single pass over the XML
struct XmlScanResult {
    /// Existing itemPath entries in HeaderFiles
    header_items: HashSet<String>,
    /// Existing itemPath entries in SourceFiles
    source_items: HashSet<String>,
    /// Insertion point for HeaderFiles
    header_insert: Option<InsertionPoint>,
    /// Insertion point for SourceFiles
    source_insert: Option<InsertionPoint>,
}

/// Scan the XML content to find existing items and insertion positions
fn scan_xml(xml_content: &str) -> Result<XmlScanResult> {
    let mut reader = Reader::from_str(xml_content);

    let mut result = XmlScanResult {
        header_items: HashSet::new(),
        source_items: HashSet::new(),
        header_insert: None,
        source_insert: None,
    };

    // State tracking
    let mut current_folder: Option<FolderKind> = None;
    let mut folder_depth: i32 = 0;
    let mut in_item_path = false;

    loop {
        match reader.read_event()? {
            Event::Start(ref e) => {
                if e.name().as_ref() == b"logicalFolder" {
                    if current_folder.is_none() {
                        if has_attr(e, "name", "HeaderFiles")? {
                            current_folder = Some(FolderKind::HeaderFiles);
                            folder_depth = 1;
                        } else if has_attr(e, "name", "SourceFiles")? {
                            current_folder = Some(FolderKind::SourceFiles);
                            folder_depth = 1;
                        }
                    } else {
                        folder_depth += 1;
                    }
                } else if current_folder.is_some() && e.name().as_ref() == b"itemPath" {
                    in_item_path = true;
                }
            }
            Event::End(ref e) => {
                if e.name().as_ref() == b"logicalFolder" && current_folder.is_some() {
                    folder_depth -= 1;
                    if folder_depth == 0 {
                        let insertion = compute_insertion_point(xml_content, &reader)?;
                        match current_folder.unwrap() {
                            FolderKind::HeaderFiles => {
                                result.header_insert = Some(insertion);
                            }
                            FolderKind::SourceFiles => {
                                result.source_insert = Some(insertion);
                            }
                        }
                        current_folder = None;
                    }
                } else if e.name().as_ref() == b"itemPath" {
                    in_item_path = false;
                }
            }
            Event::Text(ref e) => {
                if in_item_path {
                    let text = e.unescape()?.to_string();
                    match current_folder {
                        Some(FolderKind::HeaderFiles) => {
                            result.header_items.insert(text);
                        }
                        Some(FolderKind::SourceFiles) => {
                            result.source_items.insert(text);
                        }
                        None => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(result)
}

/// Compute the insertion point from the reader's current position
/// (called right after reading the `</logicalFolder>` End event).
///
/// Determines:
/// - Where to insert new lines (right after the `\n` before the closing tag's indentation)
/// - What indentation to use (matching existing `<itemPath>` entries, or closing_indent + 2 spaces)
fn compute_insertion_point(xml_content: &str, reader: &Reader<&[u8]>) -> Result<InsertionPoint> {
    let end_pos = reader.buffer_position() as usize;
    let closing_tag = "</logicalFolder>";
    let tag_start = xml_content[..end_pos]
        .rfind(closing_tag)
        .ok_or_else(|| anyhow::anyhow!("Failed to locate </logicalFolder> in XML"))?;

    // Find the last `\n` before the closing tag to determine the line start
    let line_start = xml_content[..tag_start]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(tag_start);

    // The closing tag's indentation (whitespace between `\n` and `</logicalFolder>`)
    let closing_indent = &xml_content[line_start..tag_start];

    // Try to detect item indentation from existing <itemPath> entries in this folder region.
    // Search backwards from the closing tag for the last <itemPath>.
    let item_indent = if let Some(item_pos) = xml_content[..tag_start].rfind("<itemPath>") {
        let item_line_start = xml_content[..item_pos]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        xml_content[item_line_start..item_pos].to_string()
    } else {
        // No existing items — derive from closing tag indent + 2 spaces
        format!("{}  ", closing_indent)
    };

    Ok(InsertionPoint {
        pos: line_start,
        item_indent,
    })
}

/// Check if an XML element has an attribute with the expected value
fn has_attr(element: &BytesStart, attr_name: &str, expected: &str) -> Result<bool> {
    for attr in element.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == attr_name.as_bytes() {
            return Ok(attr.unescape_value()? == expected);
        }
    }
    Ok(false)
}

/// Items to add to configurations.xml
pub struct ImportItem {
    pub filename: String,
    pub kind: FolderKind,
}

/// Update configurations.xml by adding new itemPath entries.
///
/// Returns a list of items that were actually added (excluding duplicates).
pub fn update_configurations(config_path: &Path, items: &[ImportItem]) -> Result<Vec<String>> {
    let xml_content =
        std::fs::read_to_string(config_path).context("Failed to read configurations.xml")?;

    let scan = scan_xml(&xml_content)?;

    // Separate items by kind and filter out existing ones
    let mut new_headers: Vec<&str> = Vec::new();
    let mut new_sources: Vec<&str> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for item in items {
        let already_exists = match item.kind {
            FolderKind::HeaderFiles => scan.header_items.contains(&item.filename),
            FolderKind::SourceFiles => scan.source_items.contains(&item.filename),
        };

        if already_exists {
            skipped.push(item.filename.clone());
        } else {
            match item.kind {
                FolderKind::HeaderFiles => new_headers.push(&item.filename),
                FolderKind::SourceFiles => new_sources.push(&item.filename),
            }
            added.push(item.filename.clone());
        }
    }

    for name in &skipped {
        eprintln!("  skip (already registered): {}", name);
    }

    if new_headers.is_empty() && new_sources.is_empty() {
        return Ok(added);
    }

    // Build the new XML content by splicing at insertion positions
    let mut output = xml_content.clone();

    // We need to insert from the end of the file backwards so positions stay valid
    let mut insertions: Vec<(usize, String)> = Vec::new();

    if !new_sources.is_empty() {
        let insert = scan
            .source_insert
            .ok_or_else(|| anyhow::anyhow!("SourceFiles logicalFolder not found in XML"))?;
        let block = build_item_block(&new_sources, &insert.item_indent);
        insertions.push((insert.pos, block));
    }

    if !new_headers.is_empty() {
        let insert = scan
            .header_insert
            .ok_or_else(|| anyhow::anyhow!("HeaderFiles logicalFolder not found in XML"))?;
        let block = build_item_block(&new_headers, &insert.item_indent);
        insertions.push((insert.pos, block));
    }

    // Sort by position descending so earlier insertions don't shift later ones
    insertions.sort_by(|a, b| b.0.cmp(&a.0));

    for (pos, block) in insertions {
        output.insert_str(pos, &block);
    }

    std::fs::write(config_path, &output).context("Failed to write configurations.xml")?;

    Ok(added)
}

/// Build a block of `<itemPath>` XML lines
fn build_item_block(items: &[&str], item_indent: &str) -> String {
    let mut block = String::new();
    for item in items {
        block.push_str(item_indent);
        block.push_str("<itemPath>");
        block.push_str(item);
        block.push_str("</itemPath>\n");
    }
    block
}

// ---------------------------------------------------------------------------
// Nested logical-folder support (for .xlib imports)
// ---------------------------------------------------------------------------

/// An item to import via .xlib, carrying its logical folder path.
pub struct XLibImportItem {
    pub filename: String,
    pub kind: FolderKind,
    /// e.g. ["Drivers", "CAN"] — empty means root-level
    pub logical_path: Vec<String>,
}

/// Intermediate tree built from `XLibGraftItem` entries.
/// Groups items by their logical folder hierarchy.
struct FolderTree {
    items: Vec<String>,
    children: BTreeMap<String, FolderTree>,
}

impl FolderTree {
    fn new() -> Self {
        FolderTree {
            items: Vec::new(),
            children: BTreeMap::new(),
        }
    }

    fn insert(&mut self, path: &[String], filename: String) {
        if path.is_empty() {
            self.items.push(filename);
        } else {
            self.children
                .entry(path[0].clone())
                .or_insert_with(FolderTree::new)
                .insert(&path[1..], filename);
        }
    }

    /// Render this subtree as XML string at the given indentation depth.
    fn render(&self, indent: &str, step: &str) -> String {
        let child_indent = format!("{}{}", indent, step);
        let mut out = String::new();
        for item in &self.items {
            out.push_str(&child_indent);
            out.push_str("<itemPath>");
            out.push_str(item);
            out.push_str("</itemPath>\n");
        }
        for (name, child) in &self.children {
            out.push_str(&child_indent);
            out.push_str(&format!(
                "<logicalFolder name=\"{}\" displayName=\"{}\" projectFiles=\"true\">\n",
                name, name
            ));
            out.push_str(&child.render(&child_indent, step));
            out.push_str(&child_indent);
            out.push_str("</logicalFolder>\n");
        }
        out
    }
}

/// Update configurations.xml with items that may have nested logical folder paths.
///
/// Returns the list of filenames actually added.
pub fn update_configurations_xlib(
    config_path: &Path,
    items: &[XLibImportItem],
) -> Result<Vec<String>> {
    let xml_content =
        std::fs::read_to_string(config_path).context("Failed to read configurations.xml")?;

    let scan = scan_xml(&xml_content)?;

    // Build separate folder trees for headers and sources
    let mut header_tree = FolderTree::new();
    let mut source_tree = FolderTree::new();
    let mut added: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for item in items {
        // Only check duplicates for root-level items (nested folders are new)
        let already_exists = if item.logical_path.is_empty() {
            match item.kind {
                FolderKind::HeaderFiles => scan.header_items.contains(&item.filename),
                FolderKind::SourceFiles => scan.source_items.contains(&item.filename),
            }
        } else {
            false
        };

        if already_exists {
            skipped.push(item.filename.clone());
        } else {
            match item.kind {
                FolderKind::HeaderFiles => {
                    header_tree.insert(&item.logical_path, item.filename.clone());
                }
                FolderKind::SourceFiles => {
                    source_tree.insert(&item.logical_path, item.filename.clone());
                }
            }
            added.push(item.filename.clone());
        }
    }

    for name in &skipped {
        eprintln!("  skip (already registered): {}", name);
    }

    if added.is_empty() {
        return Ok(added);
    }

    let mut output = xml_content.clone();
    let mut insertions: Vec<(usize, String)> = Vec::new();

    // Detect indent step from existing XML (default "  ")
    let step = detect_indent_step(&xml_content);

    if has_tree_content(&source_tree) {
        let insert = scan
            .source_insert
            .ok_or_else(|| anyhow::anyhow!("SourceFiles logicalFolder not found in XML"))?;
        // Derive base indent from the insertion point's item_indent minus one step
        let base_indent = insert.item_indent.strip_suffix(&step)
            .unwrap_or(&insert.item_indent);
        let block = source_tree.render(base_indent, &step);
        insertions.push((insert.pos, block));
    }

    if has_tree_content(&header_tree) {
        let insert = scan
            .header_insert
            .ok_or_else(|| anyhow::anyhow!("HeaderFiles logicalFolder not found in XML"))?;
        let base_indent = insert.item_indent.strip_suffix(&step)
            .unwrap_or(&insert.item_indent);
        let block = header_tree.render(base_indent, &step);
        insertions.push((insert.pos, block));
    }

    insertions.sort_by(|a, b| b.0.cmp(&a.0));

    for (pos, block) in insertions {
        output.insert_str(pos, &block);
    }

    std::fs::write(config_path, &output).context("Failed to write configurations.xml")?;

    Ok(added)
}

fn has_tree_content(tree: &FolderTree) -> bool {
    !tree.items.is_empty() || !tree.children.is_empty()
}

/// Detect the indent step used in the XML (e.g. "  " or "    ")
fn detect_indent_step(xml: &str) -> String {
    // Look for the first indented line to guess step
    for line in xml.lines() {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() && line.len() > trimmed.len() {
            let indent_len = line.len() - trimmed.len();
            // Find the smallest meaningful indent (likely 2 or 4 spaces)
            if indent_len <= 4 {
                return " ".repeat(indent_len);
            }
        }
    }
    "  ".to_string()
}


#[cfg(test)]
mod tests {
    use super::*;


    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<configurationDescriptor version="65">
  <logicalFolder name="root" displayName="root" projectFiles="true">
    <logicalFolder name="HeaderFiles" displayName="Header Files" projectFiles="true">
      <itemPath>main.h</itemPath>
    </logicalFolder>
    <logicalFolder name="LinkerScript" displayName="Linker Files" projectFiles="true">
    </logicalFolder>
    <logicalFolder name="SourceFiles" displayName="Source Files" projectFiles="true">
      <itemPath>main.c</itemPath>
      <itemPath>utils.c</itemPath>
    </logicalFolder>
    <logicalFolder name="ExternalFiles" displayName="Important Files" projectFiles="false">
    </logicalFolder>
  </logicalFolder>
  <confs>
    <conf name="default" type="2">
    </conf>
  </confs>
</configurationDescriptor>
"#;

    #[test]
    fn test_scan_finds_existing_items() {
        let scan = scan_xml(SAMPLE_XML).unwrap();
        assert!(scan.header_items.contains("main.h"));
        assert!(scan.source_items.contains("main.c"));
        assert!(scan.source_items.contains("utils.c"));
        assert_eq!(scan.header_items.len(), 1);
        assert_eq!(scan.source_items.len(), 2);
    }

    #[test]
    fn test_scan_finds_insert_positions() {
        let scan = scan_xml(SAMPLE_XML).unwrap();
        assert!(scan.header_insert.is_some());
        assert!(scan.source_insert.is_some());
    }

    #[test]
    fn test_build_item_block() {
        let block = build_item_block(&["can.h", "spi.h"], "      ");
        assert_eq!(
            block,
            "      <itemPath>can.h</itemPath>\n      <itemPath>spi.h</itemPath>\n"
        );
    }

    #[test]
    fn test_inject_items() {
        let dir = std::env::temp_dir().join("xgraft_test_inject");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("configurations.xml");
        std::fs::write(&config_path, SAMPLE_XML).unwrap();

        let items = vec![
            ImportItem {
                filename: "can.h".to_string(),
                kind: FolderKind::HeaderFiles,
            },
            ImportItem {
                filename: "driver.c".to_string(),
                kind: FolderKind::SourceFiles,
            },
            // Duplicate — should be skipped
            ImportItem {
                filename: "main.c".to_string(),
                kind: FolderKind::SourceFiles,
            },
        ];

        let added = update_configurations(&config_path, &items).unwrap();
        assert_eq!(added, vec!["can.h", "driver.c"]);

        let result = std::fs::read_to_string(&config_path).unwrap();
        assert!(result.contains("<itemPath>can.h</itemPath>"));
        assert!(result.contains("<itemPath>driver.c</itemPath>"));

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Verify that indentation remains consistent across multiple runs
    #[test]
    fn test_indent_consistency() {
        let dir = std::env::temp_dir().join("xgraft_test_indent");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("configurations.xml");
        std::fs::write(&config_path, SAMPLE_XML).unwrap();

        // First run: add can.h and driver.c
        let items1 = vec![
            ImportItem {
                filename: "can.h".to_string(),
                kind: FolderKind::HeaderFiles,
            },
            ImportItem {
                filename: "driver.c".to_string(),
                kind: FolderKind::SourceFiles,
            },
        ];
        update_configurations(&config_path, &items1).unwrap();

        // Second run: add spi.h and uart.c
        let items2 = vec![
            ImportItem {
                filename: "spi.h".to_string(),
                kind: FolderKind::HeaderFiles,
            },
            ImportItem {
                filename: "uart.c".to_string(),
                kind: FolderKind::SourceFiles,
            },
        ];
        update_configurations(&config_path, &items2).unwrap();

        let result = std::fs::read_to_string(&config_path).unwrap();

        // All itemPath lines should have the same indentation (6 spaces)
        for line in result.lines() {
            if line.contains("<itemPath>") {
                assert!(
                    line.starts_with("      <itemPath>"),
                    "Incorrect indentation: {:?}",
                    line
                );
            }
        }

        // The closing tags should keep their indentation (4 spaces)
        assert!(
            result.contains("    </logicalFolder>"),
            "Closing tag indentation was corrupted"
        );

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_xlib_nested_folders() {
        let dir = std::env::temp_dir().join("xgraft_test_xlib_nested");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("configurations.xml");
        std::fs::write(&config_path, SAMPLE_XML).unwrap();

        let items = vec![
            XLibImportItem {
                filename: "can.c".to_string(),
                kind: FolderKind::SourceFiles,
                logical_path: vec!["Drivers".to_string(), "CAN".to_string()],
            },
            XLibImportItem {
                filename: "can.h".to_string(),
                kind: FolderKind::HeaderFiles,
                logical_path: vec!["Drivers".to_string(), "CAN".to_string()],
            },
            XLibImportItem {
                filename: "delay.c".to_string(),
                kind: FolderKind::SourceFiles,
                logical_path: vec![],
            },
        ];

        let added = update_configurations_xlib(&config_path, &items).unwrap();
        assert_eq!(added.len(), 3);

        let result = std::fs::read_to_string(&config_path).unwrap();

        // Root-level item added directly
        assert!(result.contains("<itemPath>delay.c</itemPath>"));
        // Nested folder structure created
        assert!(result.contains("logicalFolder name=\"Drivers\""));
        assert!(result.contains("logicalFolder name=\"CAN\""));
        assert!(result.contains("<itemPath>can.c</itemPath>"));
        assert!(result.contains("<itemPath>can.h</itemPath>"));

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}
