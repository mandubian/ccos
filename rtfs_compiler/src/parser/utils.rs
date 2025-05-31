use super::PestParseError; // Import PestParseError

// Basic unescape function (replace with a proper crate if complex escapes are needed)
pub(crate) fn unescape(s: &str) -> Result<String, PestParseError> {
    // Changed error type
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some(other) => {
                    // Or return an error for invalid escapes
                    // For now, maintaining existing behavior of treating unknown escapes literally
                    // If this should be an error, it would be:
                    // return Err(PestParseError::InvalidEscapeSequence(format!("Invalid escape sequence: \\\\{}", other)));
                    result.push('\\');
                    result.push(other);
                }
                None => {
                    return Err(PestParseError::InvalidEscapeSequence(
                        "Incomplete escape sequence at end of string".to_string(),
                    ))
                } // Changed to PestParseError
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}
