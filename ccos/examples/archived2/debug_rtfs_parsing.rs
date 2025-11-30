use std::path::Path;
use std::fs;

fn parse_simple_mcp_rtfs(path: &Path) -> Option<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read RTFS file {}: {}", path.display(), e);
            return None;
        }
    };

    let extract_quoted = |key: &str| -> Option<String> {
        let start = content.find(key)? + key.len();
        let after_key = &content[start..];
        let mut chars = after_key.chars();
        let mut offset = 0;
        
        // Skip whitespace to find opening quote
        loop {
             match chars.next() {
                 Some(c) if c.is_whitespace() => offset += c.len_utf8(),
                 Some('"') => {
                     offset += 1; // skip quote
                     break;
                 }
                 _ => return None, // Found non-whitespace before quote, or EOF
             }
        }

        let after_quote = &content[start + offset..];
        let end = after_quote.find('"')?;
        let result = after_quote[..end].to_string();
        eprintln!("DEBUG: extract_quoted key='{}' found='{}'", key, result);
        Some(result)
    };

    let id = extract_quoted("(capability").or_else(|| extract_quoted(":id"));
    let name = extract_quoted(":name");
    let description = extract_quoted(":description");
    let server_url = extract_quoted(":server_url").or_else(|| extract_quoted(":server-url"));
    let tool_name = extract_quoted(":tool_name").or_else(|| extract_quoted(":tool-name"));
    
    eprintln!("DEBUG: parse_simple_mcp_rtfs path={:?}", path);
    eprintln!("DEBUG: id={:?} name={:?} server_url={:?} tool_name={:?}", id, name, server_url, tool_name);

    Some(())
}

fn main() {
    let path = Path::new("capabilities/discovered/mcp/github/github-mcp_list_issues.rtfs");
    parse_simple_mcp_rtfs(path);
}
