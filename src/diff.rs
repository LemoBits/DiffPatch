use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
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
#[derive(Debug, Clone)]
pub enum DiffType {
    Added(FileInfo),    // Added file
    Modified(FileInfo), // Modified file
    Removed(PathBuf),   // Removed file
}

/// Calculate SHA256 hash of a file
pub fn calculate_file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;
    
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;
    
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

/// Check if a file should be excluded based on exclude patterns
fn should_exclude(
    path: &Path, 
    exclude_extensions: Option<&[String]>, 
    exclude_dirs: Option<&[String]>
) -> bool {
    // Check if path has an excluded extension
    if let Some(extensions) = exclude_extensions {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let dot_ext = format!(".{}", ext);
            if extensions.iter().any(|e| e == &dot_ext || e == ext) {
                return true;
            }
        }
    }
    
    // Check if path is in an excluded directory
    if let Some(dirs) = exclude_dirs {
        let path_str = path.display().to_string();
        for dir in dirs {
            // Convert dir string into platform-specific path format
            let platform_dir = if cfg!(windows) {
                dir.replace('/', "\\")
            } else {
                dir.replace('\\', "/")
            };
            
            // Check if path contains the excluded directory
            if path_str.contains(&format!("{}{}", platform_dir, std::path::MAIN_SEPARATOR)) ||
               path_str.ends_with(&platform_dir) {
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
    exclude_dirs: Option<&[String]>
) -> Result<HashMap<PathBuf, FileInfo>> {
    let mut files = HashMap::new();
    
    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file()) {
            
        let full_path = entry.path();
        let relative_path = full_path.strip_prefix(dir_path)
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
            continue;
        }
        
        // Skip files based on exclude patterns
        if should_exclude(&relative_path, exclude_extensions, exclude_dirs) {
            continue;
        }
        
        let metadata = fs::metadata(full_path)
            .with_context(|| format!("Failed to get metadata for: {}", full_path.display()))?;
            
        let hash = calculate_file_hash(full_path)?;
        
        files.insert(
            relative_path.clone(),
            FileInfo {
                relative_path,
                hash,
                size: metadata.len(),
            },
        );
    }
    
    Ok(files)
}

/// Compare two directories and find file differences
pub fn compare_directories(
    source_dir: &Path, 
    target_dir: &Path, 
    exclude_extensions: Option<&[String]>, 
    exclude_dirs: Option<&[String]>
) -> Result<Vec<DiffType>> {
    println!("Scanning source directory: {}", source_dir.display());
    let source_files = scan_directory(source_dir, exclude_extensions, exclude_dirs)?;
    
    println!("Scanning target directory: {}", target_dir.display());
    let target_files = scan_directory(target_dir, exclude_extensions, exclude_dirs)?;
    
    let mut diffs = Vec::new();
    
    // Find modified and added files
    for (path, target_info) in &target_files {
        match source_files.get(path) {
            Some(source_info) => {
                if source_info.hash != target_info.hash {
                    diffs.push(DiffType::Modified(target_info.clone()));
                }
            },
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