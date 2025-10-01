//! Privileged helper tool for managing fingerprint authentication PAM configurations.
//!
//! This tool safely applies, removes, or checks fingerprint authentication blocks
//! in PAM configuration files for login, sudo, and polkit-1 services.

use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Markers used to fence the fingerprint configuration blocks
const BEGIN_MARK: &str = "# BEGIN xfprintd-gui";
const END_MARK: &str = "# END xfprintd-gui";

/// Standard PAM header
const PAM_HEADER: &str = "#%PAM-1.0";

/// PAM configuration file paths
const LOGIN_PATH: &str = "/etc/pam.d/login";
const SUDO_PATH: &str = "/etc/pam.d/sudo";
const POLKIT_PATH: &str = "/etc/pam.d/polkit-1";
const POLKIT_DEFAULT: &str = "/usr/lib/pam.d/polkit-1";

/// PAM configuration blocks for each service
const LOGIN_BLOCK: &str = concat!(
    "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n",
    "auth    sufficient  pam_fprintd.so",
);

const SUDO_BLOCK: &str = concat!(
    "auth    [success=1  default=ignore] pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n",
    "auth    sufficient  pam_fprintd.so",
);

const POLKIT_BLOCK: &str = concat!(
    "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n",
    "auth    sufficient  pam_fprintd.so\n",
    "auth    sufficient  pam_unix.so try_first_pass likeauth nullok",
);

/// Supported PAM configuration targets
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
enum Target {
    #[value(name = "login")]
    Login,
    #[value(name = "sudo")]
    Sudo,
    #[value(name = "polkit-1")]
    Polkit1,
}

impl Target {
    /// Returns the service name for this target
    fn service(self) -> &'static str {
        match self {
            Target::Login => "login",
            Target::Sudo => "sudo",
            Target::Polkit1 => "polkit-1",
        }
    }

    /// Returns the filesystem path for this target
    fn path(self) -> &'static str {
        get_target_path(self.service()).expect("Target must have a valid path")
    }

    /// Returns the PAM configuration block for this target
    fn config_block(self) -> &'static str {
        match self {
            Target::Login => LOGIN_BLOCK,
            Target::Sudo => SUDO_BLOCK,
            Target::Polkit1 => POLKIT_BLOCK,
        }
    }
}

/// Command line interface definition
#[derive(Debug, Parser)]
#[command(
    name = "xfprintd-gui-helper",
    version,
    about = "Apply/remove/check fingerprint PAM config blocks"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

/// Available subcommands
#[derive(Debug, Subcommand)]
enum Command {
    /// Insert fenced fingerprint block into the specified PAM target
    Apply {
        #[arg(value_enum)]
        target: Target,
    },
    /// Remove fenced fingerprint block from the specified PAM target
    Remove {
        #[arg(value_enum)]
        target: Target,
    },
    /// Exit 0 if the fenced block is present for the target, else 1
    Check {
        #[arg(value_enum)]
        target: Target,
    },
    /// Check all targets and output status for each
    CheckAll,
    /// Apply fingerprint configuration to all targets
    ApplyAll,
    /// Remove fingerprint configuration from all targets
    RemoveAll,
}

/// Maps service names to their filesystem paths
fn get_target_path(service: &str) -> Option<&'static str> {
    match service {
        "login" => Some(LOGIN_PATH),
        "sudo" => Some(SUDO_PATH),
        "polkit-1" => Some(POLKIT_PATH),
        _ => None,
    }
}

/// Checks if a path is in the allowlist of supported PAM configuration files
fn is_allowlisted_path(path: &Path) -> bool {
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return false,
    };

    [LOGIN_PATH, SUDO_PATH, POLKIT_PATH].contains(&path_str)
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

/// Applies fingerprint configuration to the specified target
fn apply_fingerprint_config(target: Target) -> io::Result<()> {
    let path = Path::new(target.path());

    if !is_allowlisted_path(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Target path is not allowlisted",
        ));
    }

    // For polkit-1, try to use system default as base if file doesn't exist
    let base_content =
        if target == Target::Polkit1 && !path.exists() && Path::new(POLKIT_DEFAULT).is_file() {
            fs::read_to_string(POLKIT_DEFAULT)?
        } else {
            read_file_or_default(path, PAM_HEADER)?
        };

    // Remove any existing blocks and insert the new one
    let cleaned_content = remove_fenced_blocks(&base_content);
    let final_content = insert_block_after_header(cleaned_content, target.config_block());

    atomic_write(path, final_content.as_bytes())
}

/// Removes fingerprint configuration from the specified target
fn remove_fingerprint_config(target: Target) -> io::Result<()> {
    let path = Path::new(target.path());

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

/// Checks if fingerprint configuration is applied to the specified target
fn is_fingerprint_applied(target: Target) -> io::Result<bool> {
    let path = Path::new(target.path());

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
        Command::Apply { target } => {
            require_root();
            match apply_fingerprint_config(target) {
                Ok(()) => println!(
                    "Success: applied fingerprint configuration to {}",
                    target.service()
                ),
                Err(e) => {
                    eprintln!(
                        "Error applying configuration to {}: {}",
                        target.service(),
                        e
                    );
                    std::process::exit(1);
                }
            }
        }

        Command::Remove { target } => {
            require_root();
            match remove_fingerprint_config(target) {
                Ok(()) => println!(
                    "Success: removed fingerprint configuration from {}",
                    target.service()
                ),
                Err(e) => {
                    eprintln!(
                        "Error removing configuration from {}: {}",
                        target.service(),
                        e
                    );
                    std::process::exit(1);
                }
            }
        }

        Command::Check { target } => match is_fingerprint_applied(target) {
            Ok(true) => {
                println!("applied: {}", target.path());
                std::process::exit(0);
            }
            Ok(false) => {
                println!("not-applied: {}", target.path());
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error checking {}: {}", target.path(), e);
                std::process::exit(2);
            }
        },

        Command::CheckAll => {
            let targets = [Target::Login, Target::Sudo, Target::Polkit1];
            let mut all_applied = true;

            for target in targets {
                match is_fingerprint_applied(target) {
                    Ok(true) => {
                        println!("applied: {}", target.path());
                    }
                    Ok(false) => {
                        println!("not-applied: {}", target.path());
                        all_applied = false;
                    }
                    Err(e) => {
                        eprintln!("Error checking {}: {}", target.path(), e);
                        std::process::exit(2);
                    }
                }
            }

            std::process::exit(if all_applied { 0 } else { 1 });
        }

        Command::ApplyAll => {
            require_root();
            let targets = [Target::Login, Target::Sudo, Target::Polkit1];
            let mut errors = Vec::new();

            for target in targets {
                if let Err(e) = apply_fingerprint_config(target) {
                    errors.push(format!("Error applying to {}: {}", target.service(), e));
                } else {
                    println!(
                        "Success: applied fingerprint configuration to {}",
                        target.service()
                    );
                }
            }

            if !errors.is_empty() {
                for error in &errors {
                    eprintln!("{}", error);
                }
                std::process::exit(1);
            }
        }

        Command::RemoveAll => {
            require_root();
            let targets = [Target::Login, Target::Sudo, Target::Polkit1];
            let mut errors = Vec::new();

            for target in targets {
                if let Err(e) = remove_fingerprint_config(target) {
                    errors.push(format!("Error removing from {}: {}", target.service(), e));
                } else {
                    println!(
                        "Success: removed fingerprint configuration from {}",
                        target.service()
                    );
                }
            }

            if !errors.is_empty() {
                for error in &errors {
                    eprintln!("{}", error);
                }
                std::process::exit(1);
            }
        }
    }
}
