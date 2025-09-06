fn main() {
    // Test the RTFS formatting functions with complex data structures
    let test_cases = vec![
        "Map({Keyword(Keyword(\"message\")): String(\"The sum is 5\")})",
        "Map({Keyword(Keyword(\"status\")): Keyword(Keyword(\"success\")), Keyword(Keyword(\"data\")): String(\"test\")})",
        "Keyword(Keyword(\"hello\"))",
        "String(\"world\")",
        "true",
        "42",
        "\"simple string\"",
    ];

    for test_case in test_cases {
        println!("Input:  {}", test_case);

        let formatted = if test_case.contains("Map(") {
            format_rtfs_map(test_case)
        } else if test_case.contains("Keyword(") {
            format_rtfs_keyword(test_case)
        } else if test_case.contains("String(") {
            format_rtfs_string(test_case)
        } else {
            test_case.to_string()
        };

        println!("Output: {}", formatted);
        println!();
    }
}

fn format_rtfs_map(map_str: &str) -> String {
    // Extract the content between Map({ and })
    if let Some(start) = map_str.find("Map({") {
        if let Some(end) = map_str.rfind("})") {
            let content = &map_str[start + 5..end]; // Skip "Map({"
            let mut result = String::from("{");
            let mut first = true;

            // Parse key-value pairs
            let pairs: Vec<&str> = content.split("), ").collect();
            for pair in pairs {
                if let Some(colon_pos) = pair.find(": ") {
                    let key_part = &pair[..colon_pos];
                    let value_part = &pair[colon_pos + 2..];

                    if !first {
                        result.push_str(", ");
                    }
                    first = false;

                    // Format key
                    if key_part.contains("Keyword(") {
                        result.push_str(&format_rtfs_keyword(key_part));
                    } else {
                        result.push_str(key_part);
                    }

                    result.push_str(" ");

                    // Format value
                    if value_part.contains("Keyword(") {
                        result.push_str(&format_rtfs_keyword(value_part));
                    } else if value_part.contains("String(") {
                        result.push_str(&format_rtfs_string(value_part));
                    } else {
                        result.push_str(value_part);
                    }
                }
            }
            result.push('}');
            return result;
        }
    }
    map_str.to_string()
}

fn format_rtfs_keyword(keyword_str: &str) -> String {
    // Parse Keyword(Keyword("content")) format and extract the inner content
    // The structure is: Keyword(Keyword("actual_keyword"))
    if let Some(first_keyword) = keyword_str.strip_prefix("Keyword(") {
        if let Some(inner_keyword) = first_keyword.strip_prefix("Keyword(") {
            if let Some(end_pos) = inner_keyword.find("\")") {
                let content = &inner_keyword[1..end_pos]; // Skip opening quote, take until closing quote
                return format!(":{}", content);
            }
        }
    }
    keyword_str.to_string()
}

fn format_rtfs_string(string_str: &str) -> String {
    // Extract content between String(") and ")
    if let Some(start) = string_str.find("String(\"") {
        if let Some(end) = string_str[start..].find("\")") {
            let content = &string_str[start + 8..start + end]; // Skip String("
            return format!("\"{}\"", content);
        }
    }
    string_str.to_string()
}
