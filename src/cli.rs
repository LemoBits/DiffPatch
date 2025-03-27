use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// File Diff Extractor - Compare directories and create executable patches
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a patch file
    Create {
        /// Source directory path
        #[arg(short, long, value_name = "DIR")]
        source: PathBuf,

        /// Target directory path
        #[arg(short, long, value_name = "DIR")]
        target: PathBuf,

        /// Output patch file path
        #[arg(short, long, value_name = "FILE")]
        output: PathBuf,

        /// Verification file list (comma-separated relative paths)
        #[arg(short, long, value_name = "FILES", value_delimiter = ',')]
        check_files: Vec<String>,

        /// Exclude file extensions (comma-separated, e.g., .tmp,.bak,.log)
        #[arg(long, value_name = "EXTENSIONS", value_delimiter = ',')]
        exclude_extensions: Option<Vec<String>>,

        /// Exclude directories (comma-separated relative paths, e.g., node_modules,dist,target)
        #[arg(long, value_name = "DIRECTORIES", value_delimiter = ',')]
        exclude_dirs: Option<Vec<String>>,
        
        /// Use file difference patches instead of storing full files (default: false)
        #[arg(long, default_value = "true")]
        use_diff_patches: bool,
    },

    /// Apply patch (typically called by the generated patch program)
    Apply {
        /// Patch data file path
        #[arg(short, long, value_name = "FILE")]
        patch_data: PathBuf,
    },
}

pub fn parse_args() -> Cli {
    Cli::parse()
} 