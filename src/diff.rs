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

/// Scan directory and collect file information
pub fn scan_directory(dir_path: &Path) -> Result<HashMap<PathBuf, FileInfo>> {
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
pub fn compare_directories(source_dir: &Path, target_dir: &Path) -> Result<Vec<DiffType>> {
    println!("Scanning source directory: {}", source_dir.display());
    let source_files = scan_directory(source_dir)?;
    
    println!("Scanning target directory: {}", target_dir.display());
    let target_files = scan_directory(target_dir)?;
    
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