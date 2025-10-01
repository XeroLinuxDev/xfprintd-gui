//! Utility functions for the XFPrintD GUI application
//!
//! This module contains simple utility functions that are used throughout
//! the application for string processing, formatting, and other common tasks.

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
