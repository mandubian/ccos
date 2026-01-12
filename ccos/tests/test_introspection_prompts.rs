use ccos::synthesis::introspection::llm_doc_parser::LlmDocParser;

#[test]
fn test_create_extraction_prompt_contains_critical_instructions() {
    let parser = LlmDocParser::new();
    let prompt = parser.create_extraction_prompt("example.com", "Some documentation content");

    println!("Generated Prompt:\n{}", prompt);

    // Verify critical base_url instruction
    assert!(
        prompt.contains("CRITICAL INSTRUCTION:\n'base_url' must be the actual API endpoint"),
        "Prompt missing critical base_url instruction"
    );

    // Verify generic example usage
    assert!(
        prompt.contains("https://api.example.com/v1"),
        "Prompt should use generic example URL"
    );

    // Verify differentiation
    assert!(
        prompt.contains("NOT the documentation URL"),
        "Prompt missing negative instruction against doc URL"
    );
}
