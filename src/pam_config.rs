use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::Command;

const LOGIN_PATH: &str = "/etc/pam.d/login";
const SUDO_PATH: &str = "/etc/pam.d/sudo";
const POLKIT_PATH: &str = "/etc/pam.d/polkit-1";

static PAM_CONFIGS: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(
        LOGIN_PATH,
        "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so",
    );

    m.insert(
        SUDO_PATH,
        "auth    [success=1  default=ignore] pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so",
    );

    m.insert(
        POLKIT_PATH,
        "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so\n\
auth    sufficient  pam_unix.so try_first_pass likeauth nullok",
    );
    m
});

pub struct PamConfig;

fn expected_config(path: &str) -> io::Result<&'static str> {
    PAM_CONFIGS
        .get(path)
        .copied()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, path.to_string()))
}

fn run_pkexec(cmd: &str) -> io::Result<()> {
    let output = Command::new("/sbin/pkexec")
        .arg("bash")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| io::Error::other(e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(io::Error::other(err));
    }

    Ok(())
}

fn escape_for_sed_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '.' | '*' | '[' | ']' | '(' | ')' | '?' | '+' | '{' | '}' | '^' | '$' | '|' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

impl PamConfig {
    pub fn is_configured(path: &str) -> bool {
        if !Path::new(path).is_file() {
            return false;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => return false,
        };

        let Ok(expected_config) = expected_config(path) else {
            return false;
        };

        expected_config.lines().all(|expected_line| {
            content
                .lines()
                .any(|file_line| file_line.trim() == expected_line.trim())
        })
    }

    pub fn apply_patch(path: &str) -> io::Result<()> {
        let content = expected_config(path)?;

        // Ensure the file exists with a standard header, then insert our block after the first line.
        let cmd = format!(
            r#"if [ ! -f '{path}' ]; then
  echo '#%PAM-1.0' > '{path}'
fi
sed -i '1r /dev/fd/3' '{path}' 3<<'PATCH'
{content}
PATCH"#
        );

        run_pkexec(&cmd)
    }

    pub fn remove_patch(path: &str) -> io::Result<()> {
        let content = expected_config(path)?;

        // Build a sed script that removes the exact lines we inserted, plus any resulting empty lines.
        let sed_script: String = content
            .lines()
            .map(|line| format!("/^{}$/d", escape_for_sed_regex(line)))
            .chain(std::iter::once("/^$/d".to_string()))
            .collect::<Vec<_>>()
            .join("\n");

        let cmd = format!(
            r#"if [ -f '{path}' ]; then
  sed -i -f /dev/fd/3 '{path}' 3<<'SED'
{script}
SED
fi"#,
            script = sed_script
        );

        run_pkexec(&cmd)
    }

    pub fn check_configurations() -> (bool, bool, bool) {
        let login = Self::is_configured(LOGIN_PATH);
        let sudo = Self::is_configured(SUDO_PATH);
        let polkit = Self::is_configured(POLKIT_PATH);
        (login, sudo, polkit)
    }

    pub fn copy_default_polkit() -> io::Result<()> {
        let cmd = format!(
            "if [ ! -f '{dst}' ] && [ -f '/usr/lib/pam.d/polkit-1' ]; then echo 'Copying default polkit config' && cp '/usr/lib/pam.d/polkit-1' '{dst}'; else echo 'polkit config exists or default missing, skipping copy'; fi",
            dst = POLKIT_PATH
        );

        run_pkexec(&cmd)
    }
}
