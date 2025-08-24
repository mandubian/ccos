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
                    // Unknown escape sequence -> return a parse error so callers can surface it
                    return Err(PestParseError::InvalidEscapeSequence {
                        sequence: format!("\\{}", other),
                        span: None,
                    });
                }
                None => {                    return Err(PestParseError::InvalidEscapeSequence { 
                        sequence: "Incomplete escape sequence at end of string".to_string(), 
                        span: None 
                    })
                } // Changed to PestParseError
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}
