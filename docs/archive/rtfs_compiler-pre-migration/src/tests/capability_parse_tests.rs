use rtfs_compiler::parser::parse_with_enhanced_errors;
use rtfs_compiler::ast::TopLevel;

#[test]
fn parse_capability_without_fence() {
    let src = r#"(capability \"travel.create-personalized-paris-itinerary\"
  :description \"Create a personalized trip itinerary for Paris based on user preferences including pace, interests, and duration\"
  :parameters {:preferences \"map\" :duration \"number\"}
  :steps (do
    (attractions.search :destination \"Paris\" :interests (get $preferences :interests))
    (itinerary.generate :attractions $found_attractions :pace (get $preferences :pace) :duration $duration)
    (itinerary.optimize :itinerary $draft_itinerary :preferences $preferences)
  )
)
"#;

    let parsed = parse_with_enhanced_errors(src, None).expect("should parse");
    assert!(parsed.iter().any(|tl| matches!(tl, TopLevel::Capability(_))));
}

#[test]
fn parse_capability_with_fence() {
    let fenced = format!("```plaintext\n{}\n```", r#"(capability \"travel.create-personalized-paris-itinerary\"
  :description \"Create a personalized trip itinerary for Paris based on user preferences including pace, interests, and duration\"
  :parameters {:preferences \"map\" :duration \"number\"}
  :steps (do
    (attractions.search :destination \"Paris\" :interests (get $preferences :interests))
    (itinerary.generate :attractions $found_attractions :pace (get $preferences :pace) :duration $duration)
    (itinerary.optimize :itinerary $draft_itinerary :preferences $preferences)
  )
)
"#);

    // Strip fences like the example code would do
    let mut inner = fenced.clone();
    if let Some(first) = inner.find("```") {
        if let Some(second_rel) = inner[first + 3..].find("```") {
            inner = inner[first + 3..first + 3 + second_rel].to_string();
        }
    }

    let parsed = parse_with_enhanced_errors(&inner, None).expect("should parse fenced content");
    assert!(parsed.iter().any(|tl| matches!(tl, TopLevel::Capability(_))));
}
