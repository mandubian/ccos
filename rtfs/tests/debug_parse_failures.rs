use rtfs::parser::parse_expression;
use std::env;
use std::fs;
use std::path::Path;

/// Extract individual test cases from a feature file (same logic as e2e harness)
fn extract_test_cases(content: &str) -> Vec<(String, String)> {
    let mut test_cases = Vec::new();
    let mut current_code = String::new();
    let mut current_expected = String::new();
    let mut in_expected = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with(";; Expected:") {
            if !current_code.trim().is_empty() {
                current_expected = trimmed
                    .trim_start_matches(";; Expected:")
                    .trim()
                    .to_string();
                in_expected = true;
            }
        } else if trimmed.starts_with(";;") || trimmed.is_empty() {
            if in_expected && !current_code.trim().is_empty() {
                test_cases.push((current_code.trim().to_string(), current_expected.clone()));
                current_code.clear();
                current_expected.clear();
                in_expected = false;
            }
            // skip comments
        } else {
            if in_expected && !current_code.trim().is_empty() {
                test_cases.push((current_code.trim().to_string(), current_expected.clone()));
                current_code.clear();
                current_expected.clear();
                in_expected = false;
            }
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    if !current_code.trim().is_empty() {
        test_cases.push((current_code.trim().to_string(), current_expected));
    }

    test_cases
}

fn read_feature(feature_name: &str) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!(
        "{}/tests/rtfs_files/features/{}.rtfs",
        manifest_dir, feature_name
    );
    if !Path::new(&path).exists() {
        return Err(format!("feature file not found: {}", path));
    }
    fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {}", path, e))
}

#[test]
fn debug_parse_selected_cases() {
    // targets to inspect
    let targets = vec![
        ("literal_values", "Special: !@#$%^&*()_+-=[]{}|;':\""),
        ("parallel_expressions", "#(* % %)"),
        ("try_catch_expressions", ".getMessage e"),
    ];

    for (feature, snippet) in targets {
        println!(
            "\n=== Inspecting feature '{}' for snippet '{}' ===",
            feature, snippet
        );
        let content = match read_feature(feature) {
            Ok(c) => c,
            Err(e) => {
                println!("  - failed to read feature: {}", e);
                continue;
            }
        };

        let cases = extract_test_cases(&content);
        let mut found = 0;

        for (idx, (code, _expected)) in cases.iter().enumerate() {
            if code.contains(snippet) {
                found += 1;
                println!("\n--- Case [{}] ---\n{}\n--- parse result ---", idx, code);
                match parse_expression(code) {
                    Ok(expr) => println!("Parsed AST: {:#?}", expr),
                    Err(e) => println!("Parse error: {:?}", e),
                }
            }
        }

        if found == 0 {
            println!("  - No test case found containing snippet '{}'", snippet);
        }
    }
}
