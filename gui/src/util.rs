/// Formats a finger name for display by replacing dashes with spaces and capitalizing the first letter.
///
/// Examples: "left-thumb" -> "Left thumb", "right-index-finger" -> "Right index finger"
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

/// Creates a shortened display name for finger labels by removing common words.
///
/// Examples: "Left index finger" -> "Index", "Right thumb" -> "Thumb"
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

/// Checks if the current Linux distribution is supported.
///
/// Examples: returns true for "XeroLinux", false for other distributions
pub fn is_supported_distribution() -> bool {
    get_distribution_name()
        .map(|name| name.to_lowercase().contains("xerolinux"))
        .unwrap_or(false)
}

/// Gets the current Linux distribution name from /etc/os-release.
///
/// Examples: returns Some("XeroLinux") on XeroLinux systems, Some("Ubuntu") on Ubuntu, etc.
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

/// Parses the NAME field from os-release file content.
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
