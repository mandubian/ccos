use std::fs;

fn main() {
    let content = fs::read_to_string("tests/rtfs_files/features/def_defn_expressions.rtfs").expect("Failed to read file");
    
    let mut test_cases = Vec::new();
    let mut current_code = String::new();
    let mut current_expected = String::new();
    let mut in_expected = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with(";; Expected:") {
            if !current_code.trim().is_empty() {
                current_expected = trimmed.trim_start_matches(";; Expected:").trim().to_string();
                in_expected = true;
            }
        } else if trimmed.starts_with(";;") && !trimmed.starts_with(";; Expected:") {
            // This is a regular comment line (not an Expected line)
            // If we have a complete test case, push it before starting a new one
            if !current_code.trim().is_empty() {
                test_cases.push((current_code.trim().to_string(), current_expected.clone()));
                current_code.clear();
                current_expected.clear();
                in_expected = false;
            }
        } else if !trimmed.is_empty() && !in_expected {
            // This is code (not a comment and not an expected value)
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    // Don't forget the last test case if there's no trailing comment
    if !current_code.trim().is_empty() {
        test_cases.push((current_code.trim().to_string(), current_expected));
    }

    // Print test case 21 (index 20)
    if test_cases.len() > 20 {
        println!("Test case 21:");
        println!("Code: {}", test_cases[20].0);
        println!("Expected: {}", test_cases[20].1);
    } else {
        println!("Test case 21 not found. Total test cases: {}", test_cases.len());
    }
}
