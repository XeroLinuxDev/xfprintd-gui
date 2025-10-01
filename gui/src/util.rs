/// Format finger name for display (replace dashes, capitalize).
pub fn display_finger_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let mut s = name.replace('-', " ");
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        let upper = first.to_ascii_uppercase().to_string();
        s.replace_range(0..first.len_utf8(), &upper);
    }
    s
}

/// Create short finger name by removing common words.
pub fn create_short_finger_name(display_name: &str) -> String {
    let mut short_name = display_name
        .replace(" finger", "")
        .replace("Left ", "")
        .replace("Right ", "");

    if let Some(first_char) = short_name.chars().next() {
        short_name =
            first_char.to_uppercase().collect::<String>() + &short_name[first_char.len_utf8()..];
    }

    short_name
}

/// Check if current distribution is supported (XeroLinux).
pub fn is_supported_distribution() -> bool {
    get_distribution_name()
        .map(|name| name.to_lowercase().contains("xerolinux"))
        .unwrap_or(false)
}

/// Get distribution name from os-release files.
pub fn get_distribution_name() -> Option<String> {
    use std::fs;

    // Try /etc/os-release first (most common)
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        if let Some(name) = parse_os_release_name(&content) {
            return Some(name);
        }
    }

    // Fallback to /usr/lib/os-release
    if let Ok(content) = fs::read_to_string("/usr/lib/os-release") {
        if let Some(name) = parse_os_release_name(&content) {
            return Some(name);
        }
    }

    // Fallback to /etc/lsb-release
    if let Ok(content) = fs::read_to_string("/etc/lsb-release") {
        for line in content.lines() {
            if line.starts_with("DISTRIB_ID=") {
                let name = line
                    .trim_start_matches("DISTRIB_ID=")
                    .trim_matches('"')
                    .to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }

    None
}

/// Parse NAME field from os-release content.
fn parse_os_release_name(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.starts_with("NAME=") {
            let name = line
                .trim_start_matches("NAME=")
                .trim_matches('"')
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}
