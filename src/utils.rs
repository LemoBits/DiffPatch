use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::path::Path;

/// Check if path exists, return error if it doesn't
pub fn check_path_exists(path: &Path, path_type: &str) -> Result<()> {
    if !path.exists() {
        Err(anyhow::anyhow!("{} does not exist: {}", path_type, path.display()))
    } else {
        Ok(())
    }
}

/// Check if path is a directory
pub fn check_is_directory(path: &Path) -> Result<()> {
    if !path.is_dir() {
        Err(anyhow::anyhow!("Path is not a directory: {}", path.display()))
    } else {
        Ok(())
    }
}

/// Interactive confirmation
pub fn confirm_action(message: &str) -> Result<bool> {
    Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .context("Failed to get user confirmation")
}