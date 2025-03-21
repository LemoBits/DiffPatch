use crate::diff::{DiffType, FileInfo};
use anyhow::{Context, Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use zip::{write::FileOptions, ZipWriter};

/// Patch data structure
#[derive(Serialize, Deserialize, Debug)]
pub struct PatchData {
    pub check_files: Vec<String>,
    pub added_files: Vec<FileInfo>,
    pub modified_files: Vec<FileInfo>,
    pub removed_files: Vec<PathBuf>,
}

impl PatchData {
    pub fn from_diffs(diffs: Vec<DiffType>, check_files: Vec<String>) -> Self {
        let mut added_files = Vec::new();
        let mut modified_files = Vec::new();
        let mut removed_files = Vec::new();

        for diff in diffs {
            match diff {
                DiffType::Added(file_info) => added_files.push(file_info),
                DiffType::Modified(file_info) => modified_files.push(file_info),
                DiffType::Removed(path) => removed_files.push(path),
            }
        }

        PatchData {
            check_files,
            added_files,
            modified_files,
            removed_files,
        }
    }
}

/// Create a patch file
pub fn create_patch(
    _source_dir: &Path,
    target_dir: &Path,
    output_file: &Path,
    diffs: Vec<DiffType>,
    check_files: Vec<String>,
) -> Result<()> {
    println!("Creating patch file: {}", output_file.display());

    // Create temporary directory to store patch data
    let temp_dir = tempdir().context("Failed to create temporary directory")?;
    let patch_data_path = temp_dir.path().join("patch_data.json");
    let content_dir = temp_dir.path().join("content");
    fs::create_dir(&content_dir).context("Failed to create content directory")?;

    // Save patch data
    let patch_data = PatchData::from_diffs(diffs, check_files);
    let patch_json = serde_json::to_string_pretty(&patch_data)
        .context("Failed to serialize patch data")?;
    fs::write(&patch_data_path, patch_json).context("Failed to write patch data")?;

    // Copy added and modified files
    let pb = ProgressBar::new((patch_data.added_files.len() + patch_data.modified_files.len()) as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message("Copying files...");

    for file_info in patch_data.added_files.iter().chain(patch_data.modified_files.iter()) {
        let source_file = target_dir.join(&file_info.relative_path);
        let dest_file = content_dir.join(&file_info.relative_path);

        // Create target directory
        if let Some(parent) = dest_file.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create directory: {}", parent.display())
            })?;
        }

        // Copy file
        fs::copy(&source_file, &dest_file).with_context(|| {
            format!(
                "Failed to copy file from {} to {}",
                source_file.display(),
                dest_file.display()
            )
        })?;

        pb.inc(1);
    }
    pb.finish_with_message("File copying complete");

    // Create ZIP archive
    let zip_path = temp_dir.path().join("patch_content.zip");
    create_zip_archive(&content_dir, &zip_path)?;

    // Get current executable path
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    
    // Copy current executable to output file
    fs::copy(&current_exe, output_file).with_context(|| {
        format!(
            "Failed to copy executable from {} to {}",
            current_exe.display(),
            output_file.display()
        )
    })?;

    // Append patch data and content to the end of executable
    append_data_to_exe(output_file, &patch_data_path, &zip_path)?;

    println!("Patch file created successfully: {}", output_file.display());
    println!("File statistics:");
    println!("  Added: {} files", patch_data.added_files.len());
    println!("  Modified: {} files", patch_data.modified_files.len());
    println!("  Deleted: {} files", patch_data.removed_files.len());

    Ok(())
}

/// Create ZIP archive
fn create_zip_archive(source_dir: &Path, zip_path: &Path) -> Result<()> {
    let file = File::create(zip_path).context("Failed to create zip file")?;
    let writer = BufWriter::new(file);
    let mut zip = ZipWriter::new(writer);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    for entry in walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(source_dir)
            .unwrap_or_else(|_| Path::new(""))
            .to_str()
            .ok_or_else(|| anyhow!("Invalid path encoding"))?;

        let mut file = File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        zip.start_file(relative_path, options).with_context(|| format!("Failed to start zip file: {}", relative_path))?;
        
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).with_context(|| format!("Failed to read file: {}", path.display()))?;
        zip.write_all(&buffer).with_context(|| format!("Failed to write to zip: {}", relative_path))?;
    }

    zip.finish().context("Failed to finish zip file")?;
    Ok(())
}

/// Append data to the end of executable file
fn append_data_to_exe(exe_path: &Path, patch_data_path: &Path, zip_path: &Path) -> Result<()> {
    let mut exe_file = fs::OpenOptions::new()
        .append(true)
        .open(exe_path)
        .with_context(|| format!("Failed to open executable file: {}", exe_path.display()))?;

    // Write patch data
    let mut patch_data = Vec::new();
    File::open(patch_data_path)
        .context("Failed to open patch data file")?
        .read_to_end(&mut patch_data)
        .context("Failed to read patch data")?;

    // Write content files
    let mut zip_data = Vec::new();
    File::open(zip_path)
        .context("Failed to open zip file")?
        .read_to_end(&mut zip_data)
        .context("Failed to read zip data")?;

    // Write end markers and offsets
    let patch_data_size = patch_data.len() as u64;
    let zip_data_size = zip_data.len() as u64;

    exe_file.write_all(&patch_data).context("Failed to write patch data to executable")?;
    exe_file.write_all(&zip_data).context("Failed to write zip data to executable")?;
    
    exe_file.write_all(&patch_data_size.to_le_bytes()).context("Failed to write patch data size")?;
    exe_file.write_all(&zip_data_size.to_le_bytes()).context("Failed to write zip data size")?;
    
    // Write magic marker
    exe_file.write_all(b"PATCH_END").context("Failed to write end marker")?;

    Ok(())
}

/// Verify if patch should be applied to the current directory
pub fn verify_directory(check_files: &[String], current_dir: &Path) -> Result<bool> {
    for file in check_files {
        let file_path = current_dir.join(file);
        if !file_path.exists() {
            println!("Verification file not found: {}", file_path.display());
            return Ok(false);
        }
    }
    Ok(true)
}

/// Extract patch data from executable
pub fn extract_patch_data_from_exe() -> Result<(PatchData, Vec<u8>)> {
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    
    let mut file = File::open(&current_exe).with_context(|| {
        format!("Failed to open executable file: {}", current_exe.display())
    })?;
    
    let file_size = file.metadata().context("Failed to get file metadata")?.len();
    
    // Check if file is large enough to contain patch data
    if file_size < 24 {  // 8 (patch_size) + 8 (zip_size) + 9 (PATCH_END)
        return Err(anyhow!("Invalid patch file: too small"));
    }
    
    // Read file end
    let mut end_marker = [0u8; 9];
    file.seek(std::io::SeekFrom::End(-9)).context("Failed to seek to end marker")?;
    file.read_exact(&mut end_marker).context("Failed to read end marker")?;
    
    if &end_marker != b"PATCH_END" {
        return Err(anyhow!("Invalid patch file: missing end marker"));
    }
    
    // Read patch data and content size
    let mut size_data = [0u8; 16];
    file.seek(std::io::SeekFrom::End(-25)).context("Failed to seek to size data")?;
    file.read_exact(&mut size_data).context("Failed to read size data")?;
    
    let patch_data_size = u64::from_le_bytes(size_data[0..8].try_into().unwrap());
    let zip_data_size = u64::from_le_bytes(size_data[8..16].try_into().unwrap());
    
    // Read patch data and content
    let offset = file_size - 25 - patch_data_size - zip_data_size;
    
    file.seek(std::io::SeekFrom::Start(offset)).context("Failed to seek to patch data")?;
    
    let mut patch_data_bytes = vec![0u8; patch_data_size as usize];
    file.read_exact(&mut patch_data_bytes).context("Failed to read patch data")?;
    
    let mut content_bytes = vec![0u8; zip_data_size as usize];
    file.read_exact(&mut content_bytes).context("Failed to read content data")?;
    
    // Deserialize patch data
    let patch_data: PatchData = serde_json::from_slice(&patch_data_bytes)
        .context("Failed to deserialize patch data")?;
    
    Ok((patch_data, content_bytes))
}

/// Apply patch to current directory
pub fn apply_patch(current_dir: &Path) -> Result<()> {
    println!("Applying patch to directory: {}", current_dir.display());
    
    // Extract patch data and content
    let (patch_data, content_bytes) = extract_patch_data_from_exe()?;
    
    // Verify if patch should be applied to this directory
    if !patch_data.check_files.is_empty() {
        println!("Verifying directory...");
        if !verify_directory(&patch_data.check_files, current_dir)? {
            return Err(anyhow!("Directory verification failed. This patch cannot be applied here."));
        }
        println!("Directory verification successful.");
    } else {
        println!("Warning: No verification files specified. Applying patch without verification.");
        if !dialoguer::Confirm::new()
            .with_prompt("Continue with patch application?")
            .default(false)
            .interact()
            .context("Failed to get user confirmation")?
        {
            return Ok(());
        }
    }
    
    // Create temporary directory to extract content
    let temp_dir = tempdir().context("Failed to create temporary directory")?;
    let zip_path = temp_dir.path().join("content.zip");
    
    // Write content to temporary file
    fs::write(&zip_path, &content_bytes).context("Failed to write content to temp file")?;
    
    // Unzip content
    let file = File::open(&zip_path).context("Failed to open zip file")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
    
    // Process files
    println!("Processing {} files...", archive.len());
    let pb = ProgressBar::new(archive.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    
    // Extract files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to access zip file entry")?;
        let outpath = match file.enclosed_name() {
            Some(path) => current_dir.join(path),
            None => {
                pb.inc(1);
                continue;
            }
        };
        
        // Create directory if needed
        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath).with_context(|| format!("Failed to create directory: {}", outpath.display()))?;
        } else {
            // Create parent directory if needed
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).with_context(|| format!("Failed to create directory: {}", parent.display()))?;
                }
            }
            // Extract file
            let mut outfile = File::create(&outpath).with_context(|| format!("Failed to create file: {}", outpath.display()))?;
            std::io::copy(&mut file, &mut outfile).with_context(|| format!("Failed to write file: {}", outpath.display()))?;
        }
        
        pb.inc(1);
        pb.set_message(format!("Extracted: {}", file.name()));
    }
    pb.finish_with_message("Files extracted successfully");
    
    // Remove files to be deleted
    if !patch_data.removed_files.is_empty() {
        println!("Removing {} files...", patch_data.removed_files.len());
        for path in &patch_data.removed_files {
            let full_path = current_dir.join(path);
            if full_path.exists() {
                fs::remove_file(&full_path).with_context(|| format!("Failed to remove file: {}", full_path.display()))?;
                println!("  Removed: {}", path.display());
            } else {
                println!("  Skip removing (not found): {}", path.display());
            }
        }
    }
    
    println!("Patch applied successfully!");
    println!("Summary:");
    println!("  Added files: {}", patch_data.added_files.len());
    println!("  Modified files: {}", patch_data.modified_files.len());
    println!("  Removed files: {}", patch_data.removed_files.len());
    
    Ok(())
} 