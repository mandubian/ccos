// Basic unescape function (replace with a proper crate if complex escapes are needed)
pub(crate) fn unescape(s: &str) -> Result<String, String> {
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
                    result.push('\\');
                    result.push(other);
                }
                None => return Err("Incomplete escape sequence".to_string()),
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}
