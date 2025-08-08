use crate::diff::{DiffChangeTag, DiffType, FileDiff, FileInfo};
use crate::utils::get_io_thread_count;
use anyhow::{Context, Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use zip::{write::FileOptions, ZipWriter};

type FileContents = Arc<Mutex<Vec<(String, Vec<u8>)>>>;


/// Patch data structure
#[derive(Serialize, Deserialize, Debug)]
pub struct PatchData {
    pub check_files: Vec<String>,
    pub added_files: Vec<FileInfo>,
    pub modified_files: Vec<FileInfo>,
    pub modified_diffs: Vec<FileDiff>,
    pub removed_files: Vec<PathBuf>,
}

impl PatchData {
    pub fn from_diffs(diffs: Vec<DiffType>, check_files: Vec<String>) -> Self {
        let mut added_files = Vec::new();
        let mut modified_files = Vec::new();
        let mut modified_diffs = Vec::new();
        let mut removed_files = Vec::new();

        for diff in diffs {
            match diff {
                DiffType::Added(file_info) => added_files.push(file_info),
                DiffType::Modified(file_info) => modified_files.push(file_info),
                DiffType::ModifiedDiff(file_diff) => modified_diffs.push(file_diff),
                DiffType::Removed(path) => removed_files.push(path),
            }
        }

        PatchData {
            check_files,
            added_files,
            modified_files,
            modified_diffs,
            removed_files,
        }
    }
}

/// Create a patch file
pub fn create_patch(
    source_dir: &Path,
    target_dir: &Path,
    output_file: &Path,
    diffs: Vec<DiffType>,
    check_files: Vec<String>,
) -> Result<()> {
    // Determine the final output path.
    // If output_file is just a filename, it will be placed in the source directory.
    // Otherwise, it will be created at the specified path.
    let mut target_output_file = if output_file.components().count() == 1 {
        source_dir.join(output_file)
    } else {
        output_file.to_path_buf()
    };

    // Ensure the output file has a .exe extension
    if target_output_file.extension().and_then(|s| s.to_str()) != Some("exe") {
        target_output_file.set_extension("exe");
    }

    println!("Creating patch file: {}", target_output_file.display());

    // Create temporary directory to store patch data
    let temp_dir = tempdir().context("Failed to create temporary directory")?;
    let patch_data_path = temp_dir.path().join("patch_data.json");
    let content_dir = temp_dir.path().join("content");
    fs::create_dir(&content_dir).context("Failed to create content directory")?;

    // Save patch data
    let patch_data = PatchData::from_diffs(diffs, check_files);
    let patch_json =
        serde_json::to_string_pretty(&patch_data).context("Failed to serialize patch data")?;
    fs::write(&patch_data_path, patch_json).context("Failed to write patch data")?;

    // Copy added and modified files
    let pb =
        ProgressBar::new((patch_data.added_files.len() + patch_data.modified_files.len()) as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    // Create a list of all files to copy
    let files_to_copy: Vec<&FileInfo> = patch_data
        .added_files
        .iter()
        .chain(patch_data.modified_files.iter())
        .collect();

    // Use atomic counter for progress
    let progress_counter = Arc::new(Mutex::new(0));

    // Perform copying in parallel
    files_to_copy.par_iter().for_each(|file_info| {
        let source_file = target_dir.join(&file_info.relative_path);
        let dest_file = content_dir.join(&file_info.relative_path);

        // Create target directory
        if let Some(parent) = dest_file.parent()
            && fs::create_dir_all(parent).is_err() {
                return; // Skip this file on error
            }

        // Copy file
        if fs::copy(&source_file, &dest_file).is_err() {
            return; // Skip this file on error
        }

        // Update progress
        let mut counter = progress_counter.lock().unwrap();
        *counter += 1;
        pb.set_position(*counter);
    });

    pb.finish_with_message("File copying complete");

    // Create ZIP archive
    let zip_path = temp_dir.path().join("patch_content.zip");
    create_zip_archive(&content_dir, &zip_path)?;

    // Get current executable path
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;

    // Copy current executable to target directory
    fs::copy(&current_exe, &target_output_file).with_context(|| {
        format!(
            "Failed to copy executable from {} to {}",
            current_exe.display(),
            target_output_file.display()
        )
    })?;

    // Append patch data and content to the end of executable
    append_data_to_exe(&target_output_file, &patch_data_path, &zip_path)?;

    println!("Patch file created successfully:");
    println!("  Location: {}", target_output_file.display());
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
    let options = FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Collect all files from the directory in parallel
    let files: Vec<_> = walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    if !files.is_empty() {
        println!("Compressing {} files...", files.len());
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        // Create a thread pool with limited threads to avoid I/O contention
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(get_io_thread_count())
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

        // Process files in parallel to prepare content
        let file_contents: FileContents =
            Arc::new(Mutex::new(Vec::with_capacity(files.len())));
        let progress_counter = Arc::new(Mutex::new(0));

        pool.install(|| {
            files.par_iter().for_each(|entry| {
                let path = entry.path();
                let relative_path = match path.strip_prefix(source_dir) {
                    Ok(rel_path) => match rel_path.to_str() {
                        Some(path_str) => path_str.to_string(),
                        None => return, // Skip files with invalid UTF-8 paths
                    },
                    Err(_) => return, // Skip if we can't get relative path
                };

                // Read file content with buffered IO
                let mut buffer = Vec::new();
                let result = (|| -> Result<(), std::io::Error> {
                    let file = File::open(path)?;
                    let mut reader = BufReader::with_capacity(65536, file);
                    reader.read_to_end(&mut buffer)?;
                    Ok(())
                })();

                if result.is_ok() {
                    let mut contents = file_contents.lock().unwrap();
                    contents.push((relative_path, buffer));

                    // Update progress
                    let mut counter = progress_counter.lock().unwrap();
                    *counter += 1;
                    pb.set_position(*counter);
                }
            });
        });

        // Extract contents from the mutex
        let contents = Arc::try_unwrap(file_contents)
            .unwrap()
            .into_inner()
            .unwrap();

        pb.finish_with_message("File reading complete");

        // Add files to the zip sequentially (ZipWriter is not thread-safe)
        println!("Creating archive...");
        let zip_pb = ProgressBar::new(contents.len() as u64);
        zip_pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        for (i, (relative_path, buffer)) in contents.into_iter().enumerate() {
            zip.start_file(&relative_path, options)
                .with_context(|| format!("Failed to start zip file: {}", relative_path))?;

            zip.write_all(&buffer)
                .with_context(|| format!("Failed to write to zip: {}", relative_path))?;

            zip_pb.set_position(i as u64 + 1);
        }

        zip_pb.finish_with_message("Archive creation complete");
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

    exe_file
        .write_all(&patch_data)
        .context("Failed to write patch data to executable")?;
    exe_file
        .write_all(&zip_data)
        .context("Failed to write zip data to executable")?;

    exe_file
        .write_all(&patch_data_size.to_le_bytes())
        .context("Failed to write patch data size")?;
    exe_file
        .write_all(&zip_data_size.to_le_bytes())
        .context("Failed to write zip data size")?;

    // Write magic marker
    exe_file
        .write_all(b"PATCH_END")
        .context("Failed to write end marker")?;

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

    let mut file = File::open(&current_exe)
        .with_context(|| format!("Failed to open executable file: {}", current_exe.display()))?;

    let file_size = file
        .metadata()
        .context("Failed to get file metadata")?
        .len();

    // Check if file is large enough to contain patch data
    if file_size < 24 {
        // 8 (patch_size) + 8 (zip_size) + 9 (PATCH_END)
        return Err(anyhow!("Invalid patch file: too small"));
    }

    // Read file end
    let mut end_marker = [0u8; 9];
    file.seek(std::io::SeekFrom::End(-9))
        .context("Failed to seek to end marker")?;
    file.read_exact(&mut end_marker)
        .context("Failed to read end marker")?;

    if &end_marker != b"PATCH_END" {
        return Err(anyhow!("Invalid patch file: missing end marker"));
    }

    // Read patch data and content size
    let mut size_data = [0u8; 16];
    file.seek(std::io::SeekFrom::End(-25))
        .context("Failed to seek to size data")?;
    file.read_exact(&mut size_data)
        .context("Failed to read size data")?;

    let patch_data_size = u64::from_le_bytes(size_data[0..8].try_into().unwrap());
    let zip_data_size = u64::from_le_bytes(size_data[8..16].try_into().unwrap());

    // Read patch data and content
    let offset = file_size - 25 - patch_data_size - zip_data_size;

    file.seek(std::io::SeekFrom::Start(offset))
        .context("Failed to seek to patch data")?;

    let mut patch_data_bytes = vec![0u8; patch_data_size as usize];
    file.read_exact(&mut patch_data_bytes)
        .context("Failed to read patch data")?;

    let mut content_bytes = vec![0u8; zip_data_size as usize];
    file.read_exact(&mut content_bytes)
        .context("Failed to read content data")?;

    // Deserialize patch data
    let patch_data: PatchData =
        serde_json::from_slice(&patch_data_bytes).context("Failed to deserialize patch data")?;

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
            return Err(anyhow!(
                "Directory verification failed. This patch cannot be applied here."
            ));
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
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    // Safely unpack the archive to a temporary location first
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_dir).context("Failed to create extraction directory")?;

    // Extract files to the temporary directory first
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .context("Failed to access zip file entry")?;
        let outpath = match file.enclosed_name() {
            Some(path) => extract_dir.join(path),
            None => {
                pb.inc(1);
                continue;
            }
        };

        // Create directory if needed
        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath)
                .with_context(|| format!("Failed to create directory: {}", outpath.display()))?;
        } else {
            // Create parent directory if needed
            if let Some(parent) = outpath.parent()
                && !parent.exists() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create directory: {}", parent.display())
                    })?;
                }
            // Extract file with buffered IO
            let mut outfile = BufWriter::with_capacity(
                65536,
                File::create(&outpath)
                    .with_context(|| format!("Failed to create file: {}", outpath.display()))?,
            );
            std::io::copy(&mut file, &mut outfile)
                .with_context(|| format!("Failed to write file: {}", outpath.display()))?;
        }

        pb.inc(1);
    }

    pb.finish_with_message("Files extracted successfully");

    // Process diff patch files
    if !patch_data.modified_diffs.is_empty() {
        println!("Applying {} file diffs...", patch_data.modified_diffs.len());
        let diff_pb = ProgressBar::new(patch_data.modified_diffs.len() as u64);
        diff_pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        // Apply diff patches one by one (no need for parallelization as each file patch operation is already fast)
        for file_diff in patch_data.modified_diffs.iter() {
            let file_path = current_dir.join(&file_diff.relative_path);

            // Check if file exists
            if !file_path.exists() {
                diff_pb.inc(1);
                continue;
            }

            // Read current file content
            let mut content = String::new();
            if let Ok(mut file) = File::open(&file_path) {
                if file.read_to_string(&mut content).is_err() {
                    // Skip if unable to read file (e.g., binary file)
                    diff_pb.inc(1);
                    continue;
                }
            } else {
                diff_pb.inc(1);
                continue;
            }

            // Split file content into lines
            let mut lines: Vec<String> = content.lines().map(|s| s.to_owned()).collect();

            // Apply changes
            // Apply changes from back to front, so line numbers aren't affected by insertions/deletions
            let mut sorted_changes = file_diff.changes.clone();
            sorted_changes.sort_by(|a, b| {
                let a_line = a.old_range.map(|(start, _)| start).unwrap_or(usize::MAX);
                let b_line = b.old_range.map(|(start, _)| start).unwrap_or(usize::MAX);
                b_line.cmp(&a_line)
            });

            for change in sorted_changes {
                match change.tag {
                    DiffChangeTag::Delete => {
                        if let Some((start, len)) = change.old_range {
                            // Ensure within range
                            if start < lines.len() {
                                let end = std::cmp::min(start + len, lines.len());
                                lines.drain(start..end);
                            }
                        }
                    }
                    DiffChangeTag::Insert => {
                        if let Some((start, _)) = change.new_range {
                            // Ensure within range
                            if start <= lines.len() {
                                let new_lines: Vec<String> =
                                    change.content.lines().map(|s| s.to_owned()).collect();
                                for (i, line) in new_lines.into_iter().enumerate() {
                                    lines.insert(start + i, line);
                                }
                            }
                        }
                    }
                    DiffChangeTag::Equal => {
                        // No changes needed for equal parts
                    }
                    DiffChangeTag::Replace => {
                        // Replace operation: delete first, then insert
                        if let Some((start, len)) = change.old_range
                            && start < lines.len() {
                                let end = std::cmp::min(start + len, lines.len());
                                lines.drain(start..end);
                            }
                        if let Some((start, _)) = change.new_range
                            && start <= lines.len() {
                                let new_lines: Vec<String> =
                                    change.content.lines().map(|s| s.to_owned()).collect();
                                for (i, line) in new_lines.into_iter().enumerate() {
                                    if start + i <= lines.len() {
                                        lines.insert(start + i, line);
                                    }
                                }
                            }
                    }
                }
            }

            // Recombine file content
            let new_content = lines.join("\n");

            // Write back to file
            if let Ok(mut file) = File::create(&file_path)
                && file.write_all(new_content.as_bytes()).is_err() {
                    // Skip on write error
                    diff_pb.inc(1);
                    continue;
                }

            diff_pb.inc(1);
        }

        diff_pb.finish_with_message("File diffs applied successfully");
    }

    // Now copy files in parallel from the temporary directory to the target directory
    let extracted_files: Vec<_> = walkdir::WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .collect();

    println!(
        "Copying {} files to target directory...",
        extracted_files.len()
    );
    let copy_pb = ProgressBar::new(extracted_files.len() as u64);
    copy_pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    // Use atomic counter for progress
    let copy_counter = Arc::new(Mutex::new(0));

    // Create a thread pool with limited threads to avoid I/O contention
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_io_thread_count())
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    // Parallel copy to target directory
    pool.install(|| {
        extracted_files.par_iter().for_each(|entry| {
            let src_path = entry.path();
            let rel_path = src_path.strip_prefix(&extract_dir).unwrap_or(src_path);
            let dest_path = current_dir.join(rel_path);

            // Ensure parent directory exists
            if let Some(parent) = dest_path.parent()
                && !parent.exists()
                    && fs::create_dir_all(parent).is_err() {
                        return; // Skip on error
                    }

            // Optimized copy with buffered IO
            let result = (|| {
                let src_file = File::open(src_path)?;
                let mut reader = BufReader::with_capacity(65536, src_file);

                let dst_file = File::create(&dest_path)?;
                let mut writer = BufWriter::with_capacity(65536, dst_file);

                std::io::copy(&mut reader, &mut writer)?;
                writer.flush()?;
                Ok::<_, std::io::Error>(())
            })();

            if result.is_err() {
                return; // Skip on error
            }

            // Update progress
            let mut counter = copy_counter.lock().unwrap();
            *counter += 1;
            copy_pb.set_position(*counter);
        });
    });

    copy_pb.finish_with_message("Files copied successfully");

    // Remove files to be deleted in parallel
    if !patch_data.removed_files.is_empty() {
        println!("Removing {} files...", patch_data.removed_files.len());

        // Use same thread pool for deletion
        pool.install(|| {
            patch_data.removed_files.par_iter().for_each(|path| {
                let full_path = current_dir.join(path);
                if full_path.exists() {
                    let _ = fs::remove_file(&full_path);
                }
            });
        });

        println!("Files removed successfully");
    }

    println!("Patch applied successfully!");
    println!("Summary:");
    println!("  Added files: {}", patch_data.added_files.len());
    println!(
        "  Modified files (full): {}",
        patch_data.modified_files.len()
    );
    println!(
        "  Modified files (diff): {}",
        patch_data.modified_diffs.len()
    );
    println!("  Removed files: {}", patch_data.removed_files.len());

    Ok(())
}
