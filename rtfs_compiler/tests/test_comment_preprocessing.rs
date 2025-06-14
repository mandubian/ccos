use std::fs;
use rtfs_compiler::*;

/// Preprocess test content to extract the actual executable expression
/// This handles files that start with comments by finding the first non-comment line
fn preprocess_test_content(content: &str) -> String {
    // Split into lines and find the first expression
    let lines: Vec<&str> = content.lines().collect();
    
    // Find the first line that starts with an opening parenthesis or other expression starter
    let mut result_lines = Vec::new();
    let mut found_expression = false;
    
    for line in lines {
        let trimmed = line.trim();
        
        // Skip empty lines and comments at the beginning
        if !found_expression && (trimmed.is_empty() || trimmed.starts_with(';')) {
            continue;
        }
        
        // Once we find the first expression, include everything from there
        found_expression = true;
        result_lines.push(line);
    }
    
    if result_lines.is_empty() {
        // If no expression found, return original content
        content.to_string()
    } else {
        result_lines.join("\n")
    }
}

#[test]
fn test_comment_preprocessing() {
    // Test with a file that starts with comments
    let test_content = "; This is a comment\n; Another comment\n\n(+ 1 2)";
    let processed = preprocess_test_content(test_content);
    println!("Original: {}", test_content);
    println!("Processed: {}", processed);
    assert_eq!(processed, "(+ 1 2)");
    
    // Test parsing the processed content
    let parsed = parser::parse_expression(&processed);
    assert!(parsed.is_ok(), "Should parse successfully: {:?}", parsed);
}

#[test]  
fn test_real_comment_file() {
    // Test with an actual file that has comments
    let test_file_path = "tests/rtfs_files/test_functions_control.rtfs";
    if std::path::Path::new(test_file_path).exists() {
        let content = fs::read_to_string(test_file_path).unwrap();
        let processed = preprocess_test_content(&content);
        
        println!("Original content first 200 chars: {}", &content[..content.len().min(200)]);
        println!("Processed content first 200 chars: {}", &processed[..processed.len().min(200)]);
        
        // Try to parse the processed content
        let parsed = parser::parse_expression(&processed);
        println!("Parse result: {:?}", parsed.is_ok());
        if let Err(e) = parsed {
            println!("Parse error: {:?}", e);
        }
    }
}
