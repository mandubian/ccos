use std::fs;
use std::path::Path;

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(next) => {
                    // Preserve unknown escape as literal backslash + char
                    out.push('\\');
                    out.push(next);
                }
                None => return Err("Invalid escape sequence: trailing backslash".to_string()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn extract_string_literals(text: &str) -> Vec<String> {
    let mut res = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            let start = i;
            let mut raw = String::new();
            while i < bytes.len() {
                let b = bytes[i];
                if b == b'\\' {
                    // include backslash and next char if present
                    raw.push('\\');
                    i += 1;
                    if i < bytes.len() {
                        raw.push(bytes[i] as char);
                        i += 1;
                    }
                } else if b == b'"' {
                    i += 1;
                    break;
                } else {
                    raw.push(b as char);
                    i += 1;
                }
            }
            res.push(raw);
        } else {
            i += 1;
        }
    }
    res
}

fn main() {
    // Try to find the test file in the crate's tests/rtfs_files directory
    let test_path = Path::new("tests/rtfs_files/test_string_ops.rtfs");
    if !test_path.exists() {
        eprintln!("Test file not found: {}", test_path.display());
        std::process::exit(2);
    }

    let text = fs::read_to_string(test_path).expect("read test file");
    println!("--- file: {} ---\n", test_path.display());
    println!("Raw file contents:\n{}\n", text);

    let literals = extract_string_literals(&text);
    if literals.is_empty() {
        println!("No string literals found.");
        return;
    }

    for (idx, raw) in literals.iter().enumerate() {
        println!("Literal #{} raw (between quotes): {}", idx + 1, raw);
        match unescape(raw) {
            Ok(u) => println!("Literal #{} unescaped: {}\n", idx + 1, u),
            Err(e) => println!("Literal #{} unescape error: {}\n", idx + 1, e),
        }
    }
}
