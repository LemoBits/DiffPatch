mod cli;
mod diff;
mod patch;
mod utils;

use anyhow::{Context, Result};
use cli::{Commands, parse_args};
use std::env;
use utils::{check_is_directory, check_path_exists};

fn main() -> Result<()> {
    // Check if running in patch mode
    if is_patch_executable() {
        println!("Running in patch mode with parallel processing...");
        let current_dir = env::current_dir().context("Failed to get current directory")?;
        return patch::apply_patch(&current_dir);
    }

    // Parse command line arguments
    let args = parse_args();

    match args.command {
        Commands::Create {
            source,
            target,
            output,
            check_files,
            exclude_extensions,
            exclude_dirs,
            use_diff_patches,
        } => {
            // Validate arguments
            check_path_exists(&source, "Source directory")
                .context("Source directory check failed")?;
            check_is_directory(&source).context("Source directory check failed")?;

            check_path_exists(&target, "Target directory")
                .context("Target directory check failed")?;
            check_is_directory(&target).context("Target directory check failed")?;

            // Display exclude patterns if specified
            if let Some(exts) = &exclude_extensions {
                if !exts.is_empty() {
                    println!("Excluding file extensions:");
                    for ext in exts {
                        println!("  - {}", ext);
                    }
                }
            }

            if let Some(dirs) = &exclude_dirs {
                if !dirs.is_empty() {
                    println!("Excluding directories:");
                    for dir in dirs {
                        println!("  - {}", dir);
                    }
                }
            }

            // Display if using diff patches
            if use_diff_patches {
                println!("Using diff patches for modified files.");
            }

            // Create patch
            let diffs = diff::compare_directories(
                &source,
                &target,
                exclude_extensions.as_deref(),
                exclude_dirs.as_deref(),
                use_diff_patches,
            )?;

            if diffs.is_empty() {
                println!("No differences found, no need to create a patch.");
                return Ok(());
            }

            let add_count = diffs
                .iter()
                .filter(|d| matches!(d, diff::DiffType::Added(_)))
                .count();
            let mod_count = diffs
                .iter()
                .filter(|d| matches!(d, diff::DiffType::Modified(_)))
                .count();
            let mod_diff_count = diffs
                .iter()
                .filter(|d| matches!(d, diff::DiffType::ModifiedDiff(_)))
                .count();
            let del_count = diffs
                .iter()
                .filter(|d| matches!(d, diff::DiffType::Removed(_)))
                .count();

            println!("Found {} file differences:", diffs.len());
            println!("  Added: {} files", add_count);
            println!("  Modified (full files): {} files", mod_count);
            if use_diff_patches {
                println!("  Modified (diff patches): {} files", mod_diff_count);
            }
            println!("  Deleted: {} files", del_count);

            // Check verification file list
            for check_file in &check_files {
                let check_path = source.join(check_file);
                if !check_path.exists() {
                    println!(
                        "Warning: Verification file does not exist: {}",
                        check_path.display()
                    );
                }
            }

            if check_files.is_empty() {
                println!(
                    "Warning: No verification files specified, patch will be applied to any directory."
                );
            } else {
                println!("Specified verification files:");
                for file in &check_files {
                    println!("  - {}", file);
                }
            }

            // Confirm patch creation
            if !utils::confirm_action("Confirm creating patch file?")? {
                println!("Operation cancelled.");
                return Ok(());
            }

            patch::create_patch(&source, &target, &output, diffs, check_files)?;
        }

        Commands::Apply { patch_data: _ } => {
            // Apply patch, typically called directly by the generated patch program, not by users
            let current_dir = env::current_dir().context("Failed to get current directory")?;
            patch::apply_patch(&current_dir)?;
        }
    }

    Ok(())
}

// Check if running as a patch executable
fn is_patch_executable() -> bool {
    // Check if the executable is large enough to contain the PATCH_END marker
    // Check if the executable ends with the PATCH_END marker
    match std::env::current_exe() {
        Ok(exe_path) => {
            match std::fs::File::open(&exe_path) {
                Ok(mut file) => {
                    use std::io::{Read, Seek, SeekFrom};
                    // Check if file is large enough to contain the marker
                    if file.metadata().map(|m| m.len() >= 9).unwrap_or(false) {
                        // Seek to the position 9 bytes from the end
                        if file.seek(SeekFrom::End(-9)).is_ok() {
                            let mut buffer = [0u8; 9];
                            // Read the last 9 bytes
                            if file.read_exact(&mut buffer).is_ok() {
                                // Compare with the marker
                                return &buffer == b"PATCH_END";
                            } else {
                                // eprintln!("Warning: Failed to read end marker from executable.");
                            }
                        } else {
                            // eprintln!("Warning: Failed to seek to end marker position in executable.");
                        }
                    } else {
                        // eprintln!("Warning: Executable too small to contain end marker.");
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to open executable to check for patch marker: {}",
                        e
                    );
                }
            }
            // If any check fails, assume it's not a patch executable
            false
        }
        Err(e) => {
            eprintln!("Warning: Failed to get current executable path: {}", e);
            false
        }
    }
}
