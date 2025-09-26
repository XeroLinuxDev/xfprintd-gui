use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::Command;

static PAM_CONFIGS: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("/etc/pam.d/login",
        "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so");

    m.insert("/etc/pam.d/sudo",
        "auth    [success=1  default=ignore] pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so");

    m.insert("/etc/pam.d/polkit-1",
        "auth    [success=1 default=ignore]  pam_succeed_if.so service in sudo:su:su-l tty in :unknown\n\
auth    sufficient  pam_fprintd.so\n\
auth    sufficient  pam_unix.so try_first_pass likeauth nullok");
    m
});

pub struct PamConfig;

impl PamConfig {
    pub fn is_configured(path: &str) -> bool {
        if !Path::new(path).exists() {
            return false;
        }

        match std::fs::read_to_string(path) {
            Ok(content) => content.contains("pam_fprintd.so"),
            Err(_) => false,
        }
    }

    pub fn apply_patch(path: &str) -> io::Result<()> {
        let content = PAM_CONFIGS
            .get(path)
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, path.to_string()))?;

        let cmd = format!(
            r#"if [ ! -f '{path}' ]; then
  echo '#%PAM-1.0' > '{path}'
fi
# Insert the provided block (via fd 3) after the first line.
sed -i '1r /dev/fd/3' '{path}' 3<<'PATCH'
{content}
PATCH"#,
            path = path,
            content = content
        );

        // eprintln!("Executing apply command for {}:\n{}", path, cmd);

        let output = Command::new("/sbin/pkexec")
            .arg("bash")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| io::Error::other(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            // eprintln!("Error applying patch to {}: {}", path, err);
            return Err(io::Error::other(err));
        }
        // if !output.stdout.is_empty() {
        //     // eprintln!(
        //     //     "Command output:\n{}",
        //     //     String::from_utf8_lossy(&output.stdout)
        //     // );
        // }
        Ok(())
    }

    pub fn remove_patch(path: &str) -> io::Result<()> {
        let content = PAM_CONFIGS
            .get(path)
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, path.to_string()))?;

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
            path = path,
            script = sed_script
        );

        // eprintln!("Executing remove command for {}:\n{}", path, cmd);

        let output = Command::new("/sbin/pkexec")
            .arg("bash")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| io::Error::other(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            // eprintln!("Error removing patch from {}: {}", path, err);
            return Err(io::Error::other(err));
        }
        // if !output.stdout.is_empty() {
        //     // eprintln!(
        //     //     "Command output:\n{}",
        //     //     String::from_utf8_lossy(&output.stdout)
        //     // );
        // }
        Ok(())
    }

    pub fn check_configurations() -> (bool, bool, bool) {
        let login = Self::is_configured("/etc/pam.d/login");
        let sudo = Self::is_configured("/etc/pam.d/sudo");
        let polkit = Self::is_configured("/etc/pam.d/polkit-1");
        (login, sudo, polkit)
    }

    pub fn copy_default_polkit() -> io::Result<()> {
        let cmd = "if [ ! -f '/etc/pam.d/polkit-1' ] && [ -f '/usr/lib/pam.d/polkit-1' ]; then echo 'Copying default polkit config' && cp '/usr/lib/pam.d/polkit-1' '/etc/pam.d/polkit-1'; else echo 'polkit config exists or default missing, skipping copy'; fi";
        // eprintln!("Executing polkit copy command:\n{}", cmd);

        let output = Command::new("/sbin/pkexec")
            .arg("bash")
            .arg("-c")
            .arg(cmd)
            .output()
            .map_err(|e| io::Error::other(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            // eprintln!("Error copying polkit config: {}", err);
            return Err(io::Error::other(err));
        }
        // if !output.stdout.is_empty() {
        //     // eprintln!(
        //     //     "Command output:\n{}",
        //     //     String::from_utf8_lossy(&output.stdout)
        //     // );
        // }
        Ok(())
    }
}
