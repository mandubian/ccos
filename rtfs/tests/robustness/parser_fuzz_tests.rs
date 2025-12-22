// Parser fuzz-style robustness tests
// Goal: ensure parsing + error formatting never panics on malformed input.

use rtfs::parser::parse_with_enhanced_errors;

fn xorshift64(mut x: u64) -> u64 {
    // Simple deterministic PRNG (no external deps)
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn gen_case(seed: u64, max_len: usize) -> String {
    let tokens: [&str; 32] = [
        "(", ")", "[", "]", "{", "}", " ", "\n", "\t", "\"", ":", ";", "let", "if", "fn", "def",
        "defn", "match", "try", "catch", "resource:ref", "nil", "true", "false", "0", "1", "42",
        "+", "-", "*", "/", "x",
    ];

    let mut out = String::new();
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15u64;
    let target_len = (state as usize % max_len).max(1);

    while out.len() < target_len {
        state = xorshift64(state);
        let tok = tokens[(state as usize) % tokens.len()];
        out.push_str(tok);
    }
    out
}

#[test]
fn test_parser_never_panics_on_generated_inputs() {
    // Keep it small enough to stay fast in CI, but large enough to catch regressions.
    const CASES: u64 = 300;
    const MAX_LEN: usize = 256;

    for seed in 0..CASES {
        let input = gen_case(seed, MAX_LEN);

        // Parsing should never panic.
        let parse_result = std::panic::catch_unwind(|| parse_with_enhanced_errors(&input, Some("fuzz.rtfs")));
        assert!(
            parse_result.is_ok(),
            "parse_with_enhanced_errors panicked for seed {} input {:?}",
            seed,
            input
        );

        // Formatting should also never panic (even for weird parse failures).
        if let Ok(Err(err)) = parse_result {
            let formatted = std::panic::catch_unwind(|| err.format_with_context());
            assert!(
                formatted.is_ok(),
                "format_with_context panicked for seed {} input {:?}",
                seed,
                input
            );
            let formatted = formatted.unwrap();
            assert!(
                formatted.contains("Parse Error"),
                "formatted error should contain header; got:\n{}",
                formatted
            );
        }
    }
}


