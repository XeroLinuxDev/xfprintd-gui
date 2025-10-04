//! Privileged helper tool for managing PAM configurations using patch files.
//!
//! This tool safely applies, removes, or checks configuration blocks
//! in PAM configuration files using patch files stored alongside the binary.
//!
//! Patch files are stored in: /opt/xfprintd-gui/patches/<encoded-path>.patch
//! For example: /opt/xfprintd-gui/patches/etc/pam.d/sudo.patch

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Markers used to fence the configuration blocks
const BEGIN_MARK: &str = "# BEGIN xfprintd-gui";
const END_MARK: &str = "# END xfprintd-gui";

/// Standard PAM header
const PAM_HEADER: &str = "#%PAM-1.0";

/// Base directory for patches (relative to binary location)
const PATCHES_BASE_DIR: &str = "/opt/xfprintd-gui/patches";

/// Allowlisted PAM configuration directories
const ALLOWED_DIRS: &[&str] = &["/etc/pam.d"];

/// Target configuration with optional default file fallback
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetConfig {
    /// Target file path (e.g., "/etc/pam.d/sudo")
    file: String,
    /// Optional default file to use if target doesn't exist (e.g., "/usr/lib/pam.d/polkit-1")
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<String>,
}

impl TargetConfig {
    fn new(file: String) -> Self {
        Self {
            file,
            default: None,
        }
    }

    fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Command line interface definition
#[derive(Debug, Parser)]
#[command(
    name = "xfprintd-gui-helper",
    version,
    about = "Apply/remove/check PAM config blocks using patch files"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

/// Available subcommands
#[derive(Debug, Subcommand)]
enum Command {
    /// Insert fenced configuration block into specified PAM files
    Apply {
        /// JSON objects with 'file' and optional 'default' fields
        /// Example: '{"file":"/etc/pam.d/sudo"}' or '{"file":"/etc/pam.d/polkit-1","default":"/usr/lib/pam.d/polkit-1"}'
        #[arg(required = true)]
        targets: Vec<String>,
    },
    /// Remove fenced configuration block from specified PAM files
    Remove {
        /// PAM configuration file paths (e.g., /etc/pam.d/sudo)
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Check if configuration is applied to specified PAM files
    Check {
        /// PAM configuration file paths (e.g., /etc/pam.d/sudo)
        #[arg(required = true)]
        paths: Vec<String>,
    },
}

/// Converts a file path to its corresponding patch file path
/// Example: /etc/pam.d/sudo -> /opt/xfprintd-gui/patches/etc/pam.d/sudo.patch
fn get_patch_path(target_path: &str) -> PathBuf {
    let normalized = target_path.strip_prefix('/').unwrap_or(target_path);
    PathBuf::from(PATCHES_BASE_DIR)
        .join(normalized)
        .with_extension("patch")
}

/// Checks if a path is in the allowlist of supported PAM configuration directories
fn is_allowlisted_path(path: &Path) -> bool {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return false,
    };

    ALLOWED_DIRS
        .iter()
        .any(|allowed| path_str.starts_with(allowed))
}

/// Reads patch file content for the given target path
fn read_patch_content(target_path: &str) -> io::Result<String> {
    let patch_path = get_patch_path(target_path);

    if !patch_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Patch file not found: {}", patch_path.display()),
        ));
    }

    let content = fs::read_to_string(&patch_path)?;

    // Remove trailing newlines/whitespace for consistent formatting
    Ok(content.trim_end().to_string())
}

/// Creates a fenced configuration block with begin/end markers
fn create_fenced_block(content: &str) -> String {
    format!("{}\n{}\n{}\n", BEGIN_MARK, content, END_MARK)
}

/// Reads a file to string, or returns a default value if the file doesn't exist
fn read_file_or_default(path: &Path, default: &str) -> io::Result<String> {
    if path.exists() {
        fs::read_to_string(path)
    } else {
        Ok(format!("{}\n", default))
    }
}

/// Removes any existing fenced blocks from the content
fn remove_fenced_blocks(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut inside_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == BEGIN_MARK {
            inside_block = true;
            continue;
        }

        if trimmed == END_MARK {
            inside_block = false;
            continue;
        }

        if !inside_block {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

/// Inserts a configuration block after the first line (typically the PAM header)
fn insert_block_after_header(mut base_content: String, block: &str) -> String {
    // Ensure content ends with newline for predictable processing
    if !base_content.ends_with('\n') {
        base_content.push('\n');
    }

    let lines: Vec<&str> = base_content.lines().collect();
    let fenced_block = create_fenced_block(block);
    let mut result = String::with_capacity(base_content.len() + fenced_block.len() + 32);

    if lines.is_empty() {
        // No existing content - add PAM header then the block
        result.push_str(PAM_HEADER);
        result.push('\n');
        result.push_str(&fenced_block);
    } else {
        // Preserve first line (usually PAM header), insert block, then remaining lines
        result.push_str(lines[0]);
        result.push('\n');
        result.push_str(&fenced_block);

        if lines.len() > 1 {
            for line in &lines[1..] {
                result.push_str(line);
                result.push('\n');
            }
        }
    }

    result
}

/// Atomically writes data to a file using a temporary file and rename
fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Path has no parent directory")
    })?;

    // Generate unique temporary filename
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("xfprintd-gui");
    let temp_name = format!(".{}.{}-{}.tmp", file_name, pid, timestamp);
    let temp_path = parent.join(temp_name);

    // Preserve existing file permissions or use default 0644
    let mode = if path.exists() {
        fs::metadata(path)?.permissions().mode()
    } else {
        0o644
    };

    // Write to temporary file
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;
        file.write_all(data)?;
        file.sync_all()?;
    }

    // Set permissions and atomically replace
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(mode))?;
    fs::rename(&temp_path, path)?;

    // Sync directory for durability
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}

/// Applies configuration to the specified target
fn apply_config(target: &TargetConfig) -> io::Result<()> {
    let path = Path::new(&target.file);

    if !is_allowlisted_path(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Target path is not allowlisted: {}", target.file),
        ));
    }

    // Read the patch content
    let patch_content = read_patch_content(&target.file)?;

    // Use default file if specified and target doesn't exist
    let base_content = if !path.exists() {
        if let Some(default_path) = &target.default {
            let default = Path::new(default_path);
            if default.is_file() {
                fs::read_to_string(default)?
            } else {
                read_file_or_default(path, PAM_HEADER)?
            }
        } else {
            read_file_or_default(path, PAM_HEADER)?
        }
    } else {
        read_file_or_default(path, PAM_HEADER)?
    };

    // Remove any existing blocks and insert the new one
    let cleaned_content = remove_fenced_blocks(&base_content);
    let final_content = insert_block_after_header(cleaned_content, &patch_content);

    atomic_write(path, final_content.as_bytes())
}

/// Removes configuration from the specified target path
fn remove_config(target_path: &str) -> io::Result<()> {
    let path = Path::new(target_path);

    if !path.exists() || !is_allowlisted_path(path) {
        return Ok(()); // Nothing to do
    }

    let original_content = fs::read_to_string(path)?;
    let cleaned_content = remove_fenced_blocks(&original_content);

    // Only write if content changed
    if cleaned_content != original_content {
        atomic_write(path, cleaned_content.as_bytes())?;
    }

    Ok(())
}

/// Checks if configuration is applied to the specified target path
fn is_config_applied(target_path: &str) -> io::Result<bool> {
    let path = Path::new(target_path);

    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)?;
    Ok(content.contains(BEGIN_MARK))
}

/// Checks if the current process is running as root
fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Requires root privileges for the operation, exits with error code 126 if not root
fn require_root() {
    if !is_root() {
        eprintln!("Permission denied: must be run as root (via pkexec)");
        std::process::exit(126);
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.cmd {
        Command::Apply { targets } => {
            require_root();
            let mut errors = Vec::new();

            for target_str in &targets {
                // Try to parse as JSON first
                let target = match TargetConfig::from_json(target_str) {
                    Ok(t) => t,
                    Err(_) => {
                        // If JSON parsing fails, treat as simple file path for backwards compatibility
                        TargetConfig::new(target_str.clone())
                    }
                };

                match apply_config(&target) {
                    Ok(()) => println!("Success: applied configuration to {}", target.file),
                    Err(e) => {
                        let error =
                            format!("Error applying configuration to {}: {}", target.file, e);
                        eprintln!("{}", error);
                        errors.push(error);
                    }
                }
            }

            if !errors.is_empty() {
                std::process::exit(1);
            }
        }

        Command::Remove { paths } => {
            require_root();
            let mut errors = Vec::new();

            for path in &paths {
                match remove_config(path) {
                    Ok(()) => println!("Success: removed configuration from {}", path),
                    Err(e) => {
                        let error = format!("Error removing configuration from {}: {}", path, e);
                        eprintln!("{}", error);
                        errors.push(error);
                    }
                }
            }

            if !errors.is_empty() {
                std::process::exit(1);
            }
        }

        Command::Check { paths } => {
            let mut all_applied = true;

            for path in &paths {
                match is_config_applied(path) {
                    Ok(true) => {
                        println!("applied: {}", path);
                    }
                    Ok(false) => {
                        println!("not-applied: {}", path);
                        all_applied = false;
                    }
                    Err(e) => {
                        eprintln!("Error checking {}: {}", path, e);
                        std::process::exit(2);
                    }
                }
            }

            std::process::exit(if all_applied { 0 } else { 1 });
        }
    }
}
