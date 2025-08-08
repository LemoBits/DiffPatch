use crate::utils::get_io_thread_count;
use anyhow::{Context, Result};
use log::info;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// File information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub relative_path: PathBuf,
    pub hash: String,
    pub size: u64,
}

/// File difference types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiffType {
    Added(FileInfo),        // Added file
    Modified(FileInfo),     // Modified file with full content
    ModifiedDiff(FileDiff), // Modified file with only the differences
    Removed(PathBuf),       // Removed file
}

/// Structure to hold file differences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub relative_path: PathBuf,
    pub hash: String,             // hash of target file
    pub original_hash: String,    // hash of source file
    pub changes: Vec<DiffChange>, // changes to apply
}

/// Structure to represent a single change in a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffChange {
    pub tag: DiffChangeTag,
    pub content: String,
    pub old_range: Option<(usize, usize)>, // start line, length
    pub new_range: Option<(usize, usize)>, // start line, length
}

/// Tags to represent different types of changes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiffChangeTag {
    Equal,
    Delete,
    Insert,
    Replace,
}

/// Calculate SHA256 hash of a file with buffered reading
pub fn calculate_file_hash(path: &Path) -> Result<String> {
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;

    // Use a buffered reader for better I/O performance
    let mut reader = BufReader::with_capacity(65536, file); // 64KB buffer

    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

/// Check if a file should be excluded based on exclude patterns
fn should_exclude(
    path: &Path,
    exclude_extensions: Option<&[String]>,
    exclude_dirs: Option<&[String]>,
) -> bool {
    // Check if path has an excluded extension
    if let Some(extensions) = exclude_extensions
        && let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let dot_ext = format!(".{}", ext);
            if extensions.iter().any(|e| e == &dot_ext || e == ext) {
                return true;
            }
        }

    // Check if the path is within an excluded directory
    if let Some(dirs) = exclude_dirs {
        let mut path_ancestors = path.ancestors();
        // Skip the first ancestor, which is the path itself
        path_ancestors.next();

        for ancestor in path_ancestors {
            if let Some(dir_name) = ancestor.file_name().and_then(|n| n.to_str())
                && dirs.iter().any(|excluded_dir| excluded_dir == dir_name) {
                    return true;
                }
        }
    }

    false
}

/// Scan directory and collect file information
pub fn scan_directory(
    dir_path: &Path,
    exclude_extensions: Option<&[String]>,
    exclude_dirs: Option<&[String]>,
) -> Result<HashMap<PathBuf, FileInfo>> {
    // Collect all valid files first
    let files_to_process: Vec<_> = WalkDir::new(dir_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let full_path = e.path();
            let relative_path = full_path
                .strip_prefix(dir_path)
                .unwrap_or_else(|_| Path::new(""))
                .to_path_buf();

            // Skip hidden files and directories
            if relative_path.components().any(|c| {
                if let Some(s) = c.as_os_str().to_str() {
                    s.starts_with('.')
                } else {
                    false
                }
            }) {
                return false;
            }

            // Skip files based on exclude patterns
            !should_exclude(&relative_path, exclude_extensions, exclude_dirs)
        })
        .collect();

    // Create a thread pool with limited threads to avoid I/O contention
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_io_thread_count())
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    // Process files in parallel with the custom thread pool
    let results = pool.install(|| {
        files_to_process
            .par_iter()
            .map(|entry| {
                let full_path = entry.path();
                let relative_path = match full_path.strip_prefix(dir_path) {
                    Ok(path) => path.to_path_buf(),
                    Err(_) => return None,
                };

                // Get metadata
                let metadata = match fs::metadata(full_path) {
                    Ok(meta) => meta,
                    Err(_) => return None,
                };

                // Calculate hash
                let hash = match calculate_file_hash(full_path) {
                    Ok(h) => h,
                    Err(_) => return None,
                };

                Some((
                    relative_path.clone(),
                    FileInfo {
                        relative_path,
                        hash,
                        size: metadata.len(),
                    },
                ))
            })
            .collect::<Vec<_>>()
    });

    // Add results to HashMap
    let mut files_map = HashMap::with_capacity(results.len());
    for result in results.into_iter().flatten() {
        files_map.insert(result.0, result.1);
    }

    Ok(files_map)
}

/// Calculate file differences between two files
pub fn calculate_file_diff(
    source_path: &Path,
    target_path: &Path,
    relative_path: &Path,
) -> Result<FileDiff> {
    // Read source file content
    let mut source_content = String::new();
    let mut source_file = fs::File::open(source_path).with_context(|| {
        format!(
            "Failed to open source file for diff: {}",
            source_path.display()
        )
    })?;
    source_file
        .read_to_string(&mut source_content)
        .with_context(|| {
            format!(
                "Failed to read source file for diff: {}",
                source_path.display()
            )
        })?;

    // Read target file content
    let mut target_content = String::new();
    let mut target_file = fs::File::open(target_path).with_context(|| {
        format!(
            "Failed to open target file for diff: {}",
            target_path.display()
        )
    })?;
    target_file
        .read_to_string(&mut target_content)
        .with_context(|| {
            format!(
                "Failed to read target file for diff: {}",
                target_path.display()
            )
        })?;

    // Calculate hashes
    let source_hash = calculate_file_hash(source_path)?;
    let target_hash = calculate_file_hash(target_path)?;

    // Calculate diff
    let diff = TextDiff::from_lines(&source_content, &target_content);

    let mut changes = Vec::new();

    for group in diff.grouped_ops(3).iter() {
        for op in group {
            // Use the operations directly instead of iter_inline_changes
            let (old_start, old_len) = (op.old_range().start, op.old_range().len());
            let (new_start, new_len) = (op.new_range().start, op.new_range().len());

            // Get old and new slices
            let old_lines: Vec<&str> = source_content
                .lines()
                .skip(old_start)
                .take(old_len)
                .collect();
            let new_lines: Vec<&str> = target_content
                .lines()
                .skip(new_start)
                .take(new_len)
                .collect();

            // Create changes based on operation type
            if old_len > 0 && new_len > 0 {
                // Replace
                changes.push(DiffChange {
                    tag: DiffChangeTag::Replace,
                    content: new_lines.join("\n"),
                    old_range: Some((old_start, old_len)),
                    new_range: Some((new_start, new_len)),
                });
            } else if old_len > 0 {
                // Delete
                changes.push(DiffChange {
                    tag: DiffChangeTag::Delete,
                    content: old_lines.join("\n"),
                    old_range: Some((old_start, old_len)),
                    new_range: None,
                });
            } else if new_len > 0 {
                // Insert
                changes.push(DiffChange {
                    tag: DiffChangeTag::Insert,
                    content: new_lines.join("\n"),
                    old_range: None,
                    new_range: Some((new_start, new_len)),
                });
            }
        }
    }

    // Create the file diff structure
    let file_diff = FileDiff {
        relative_path: relative_path.to_path_buf(),
        hash: target_hash,
        original_hash: source_hash,
        changes,
    };

    Ok(file_diff)
}

/// Compare two directories and find file differences
pub fn compare_directories(
    source_dir: &Path,
    target_dir: &Path,
    exclude_extensions: Option<&[String]>,
    exclude_dirs: Option<&[String]>,
    use_diff_patches: bool, // Add parameter to control whether to use diff patches
) -> Result<Vec<DiffType>> {
    info!("Scanning source directory: {}", source_dir.display());
    let source_files = scan_directory(source_dir, exclude_extensions, exclude_dirs)?;

    info!("Scanning target directory: {}", target_dir.display());
    let target_files = scan_directory(target_dir, exclude_extensions, exclude_dirs)?;

    let mut diffs = Vec::new();

    // Find modified and added files
    for (path, target_info) in &target_files {
        match source_files.get(path) {
            Some(source_info) => {
                if source_info.hash != target_info.hash {
                    if use_diff_patches {
                        // Check if it's a text file that we can diff
                        let source_path = source_dir.join(path);
                        let target_path = target_dir.join(path);

                        // Try to create a diff
                        match calculate_file_diff(&source_path, &target_path, path) {
                            Ok(file_diff) => {
                                diffs.push(DiffType::ModifiedDiff(file_diff));
                            }
                            Err(_) => {
                                // If diff fails (e.g., binary file), fall back to full file
                                diffs.push(DiffType::Modified(target_info.clone()));
                            }
                        }
                    } else {
                        // Use full file mode
                        diffs.push(DiffType::Modified(target_info.clone()));
                    }
                }
            }
            None => {
                diffs.push(DiffType::Added(target_info.clone()));
            }
        }
    }

    // Find removed files
    for path in source_files.keys() {
        if !target_files.contains_key(path) {
            diffs.push(DiffType::Removed(path.clone()));
        }
    }

    Ok(diffs)
}
