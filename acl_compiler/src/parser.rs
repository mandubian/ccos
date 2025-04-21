use nom::{
    branch::alt,
    // Removed is_not
    bytes::complete::{tag, take_while, take_while1, take_while_m_n},
    // Removed unused line_ending, multispace0
    character::complete::{char, multispace1, none_of, not_line_ending},
    combinator::{map, map_res, opt, recognize, value},
    error::ParseError,
    multi::{many0, many1, separated_list0}, // Added many1
    sequence::{delimited, pair, preceded, separated_pair, tuple},
    IResult,
};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;

use crate::ast::{Expr, MapKey, Value}; // Added MapKey

// Parser for single-line comments starting with ;
fn parse_comment<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    recognize(preceded(char(';'), opt(not_line_ending)))(input)
}

// Parser for zero or more whitespace characters or comments
fn ws_or_comment0<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    recognize(many0(alt((multispace1, parse_comment))))(input)
}

// Parser for one or more whitespace characters or comments
fn ws_or_comment1<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, &'a str, E> {
    recognize(many1(alt((multispace1, parse_comment))))(input) // Use many1 here
}

// Helper function to consume whitespace OR comments
fn ws<'a, F, O, E: ParseError<&'a str>>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    delimited(ws_or_comment0, inner, ws_or_comment0) // Use ws_or_comment0
}

// Parser for Nil
fn parse_nil(input: &str) -> IResult<&str, Value> {
    map(tag("nil"), |_| Value::Nil)(input)
}

// Parser for Booleans
fn parse_bool(input: &str) -> IResult<&str, Value> {
    alt((
        value(Value::Bool(true), tag("true")),
        value(Value::Bool(false), tag("false")),
    ))(input)
}

// Parser for Integers (Ensure it doesn't parse floats)
fn parse_int(input: &str) -> IResult<&str, Value> {
    // Use recognize first to capture the potential integer part
    let (remaining, recognized) =
        recognize(pair(opt(char('-')), take_while1(|c: char| c.is_digit(10))))(input)?;

    // Check the character immediately after the recognized part in the original input
    match remaining.chars().next() {
        // If the next char indicates a float, fail parsing the integer
        Some('.') | Some('e') | Some('E') => {
            Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            ))) // Indicate verification failure
        }
        // Otherwise, proceed to convert the recognized part to BigInt
        _ => {
            match BigInt::from_str(recognized) {
                Ok(val) => Ok((remaining, Value::Int(val))),
                Err(_) => Err(nom::Err::Error(nom::error::Error::new(
                    recognized,
                    nom::error::ErrorKind::MapRes,
                ))), // Error during BigInt conversion
            }
        }
    }
}

// Parser for Floats (recognizes the pattern first)
// Handles forms like: 1.23, -0.5, 1., .5, 1e5, 1.2e-3, -inf, +inf, nan
// *** Must contain '.' or 'e'/'E' to distinguish from integers ***

// Helper parser for standard float patterns (excluding inf/nan)
fn parse_standard_float_pattern(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        opt(alt((char('+'), char('-')))), // Optional sign
        alt((
            // Case 1: Digits + '.' + Optional Digits + Optional Exponent
            recognize(tuple((
                take_while1(|c: char| c.is_digit(10)),
                char('.'),
                take_while(|c: char| c.is_digit(10)), // Optional digits after dot
                opt(tuple((
                    // Optional exponent
                    alt((char('e'), char('E'))),
                    opt(alt((char('+'), char('-')))),
                    take_while1(|c: char| c.is_digit(10)),
                ))),
            ))),
            // Case 2: '.' + Digits + Optional Exponent
            recognize(tuple((
                char('.'),
                take_while1(|c: char| c.is_digit(10)),
                opt(tuple((
                    // Optional exponent
                    alt((char('e'), char('E'))),
                    opt(alt((char('+'), char('-')))),
                    take_while1(|c: char| c.is_digit(10)),
                ))),
            ))),
            // Case 3: Digits + Exponent (No decimal point)
            recognize(tuple((
                take_while1(|c: char| c.is_digit(10)),
                // Exponent MUST be present here
                tuple((
                    alt((char('e'), char('E'))),
                    opt(alt((char('+'), char('-')))),
                    take_while1(|c: char| c.is_digit(10)),
                )),
            ))),
        )),
    ))(input)
}

fn parse_float(input: &str) -> IResult<&str, Value> {
    map_res(
        alt((
            // Handle inf, -inf, nan first as they are distinct patterns
            tag("inf"),
            tag("-inf"),
            tag("+inf"),
            tag("nan"),
            // Use the helper for standard float patterns
            parse_standard_float_pattern,
        )),
        |s: &str| {
            // Same mapping logic as before
            match s {
                "inf" | "+inf" => Ok(Value::Float(f64::INFINITY)),
                "-inf" => Ok(Value::Float(f64::NEG_INFINITY)),
                "nan" => Ok(Value::Float(f64::NAN)),
                _ => s.parse::<f64>().map(Value::Float),
            }
        },
    )(input)
}

// Helper for parsing a single escaped character
fn parse_escape_sequence(input: &str) -> IResult<&str, char> {
    preceded(
        char('\\'),
        alt((
            value('\\', tag("\\")),
            value('\"', tag("\"")),
            value('\n', tag("n")),
            value('\r', tag("r")),
            value('\t', tag("t")),
            // TODO: Add more escapes like unicode? \u{...}
        )),
    )(input)
}

// Helper for parsing a single non-escape, non-quote character
fn parse_normal_char(input: &str) -> IResult<&str, char> {
    none_of("\\\"")(input)
}

// Parser for string content (handles escapes)
fn parse_string_content(input: &str) -> IResult<&str, String> {
    map(
        many0(alt((parse_escape_sequence, parse_normal_char))),
        |chars: Vec<char>| chars.into_iter().collect(),
    )(input)
}

// Parser for Strings using the manual content parser
fn parse_string(input: &str) -> IResult<&str, Value> {
    map(
        delimited(
            char('"'),
            parse_string_content, // Use the new content parser
            char('"'),
        ),
        Value::String,
    )(input)
}

// Parser for Symbols / Keywords
// Must not be nil, true, false, or a number.
// Starts with non-digit, non-colon, non-quote, non-paren/bracket/brace, non-whitespace.
// Continues with non-quote, non-paren/bracket/brace, non-whitespace.
fn is_symbol_start_char(c: char) -> bool {
    // Exclude digits from starting characters
    c.is_alphabetic() || "*+!-_?><=/~@$%^&".contains(c)
}

fn is_symbol_char(c: char) -> bool {
    // Allow digits inside, but not at the start (handled by is_symbol_start_char)
    is_symbol_start_char(c) || c.is_digit(10) || ".#:".contains(c)
}

fn parse_symbol(input: &str) -> IResult<&str, Value> {
    map_res(
        recognize(tuple((
            take_while_m_n(1, 1, is_symbol_start_char),
            take_while(is_symbol_char),
        ))),
        |s: &str| {
            // Check if it's a reserved keyword or number pattern that failed other parsers
            match s {
                "nil" | "true" | "false" => Err("Reserved keyword used as symbol"), // Should have been parsed by parse_nil/parse_bool
                _ => {
                    // Basic check to prevent parsing numbers as symbols
                    if s.chars().all(|c| c.is_digit(10) || c == '-' || c == '.')
                        && s.parse::<f64>().is_ok()
                    {
                        Err("Numeric literal used as symbol")
                    } else {
                        Ok(Value::Symbol(s.to_string()))
                    }
                }
            }
        },
    )(input)
}

// Parser for Keywords (e.g., :my-keyword)
fn parse_keyword(input: &str) -> IResult<&str, Value> {
    map(
        preceded(
            char(':'),
            // Use the updated symbol character rules
            recognize(tuple((
                take_while_m_n(1, 1, is_symbol_start_char),
                take_while(is_symbol_char),
            ))),
        ),
        |s: &str| Value::Keyword(s.to_string()),
    )(input)
}

// Parser for Lists: (item1 item2 ...) - WITHOUT outer ws
fn parse_list_internal(input: &str) -> IResult<&str, Value> {
    map(
        delimited(
            ws(char('(')), // Keep ws around opening delimiter
            ws(separated_list0(
                ws_or_comment1,       // Use ws_or_comment1 as separator
                parse_value_internal, // Element parser itself doesn't consume surrounding ws
            )),
            char(')'), // Apply ws to closing delimiter
        ),
        Value::List,
    )(input)
}

// Parser for Vectors: [item1 item2 ...] - WITHOUT outer ws
fn parse_vector_internal(input: &str) -> IResult<&str, Value> {
    map(
        delimited(
            ws(char('[')), // Keep ws around opening delimiter
            ws(separated_list0(
                ws_or_comment1,       // Use ws_or_comment1 as separator
                parse_value_internal, // Element parser itself doesn't consume surrounding ws
            )),
            char(']'), // Apply ws to closing delimiter
        ),
        Value::Vector,
    )(input)
}

// Parser for Map Entries: key value - WITHOUT outer ws
fn parse_map_entry_internal(input: &str) -> IResult<&str, (MapKey, Value)> {
    map_res(
        separated_pair(
            parse_value_internal, // Key parser
            ws_or_comment1,       // Use ws_or_comment1 as separator
            parse_value_internal, // Value parser without surrounding ws
        ),
        |(k, v)| {
            k.into_map_key()
                .ok_or("Invalid map key type")
                .map(|map_key| (map_key, v))
        },
    )(input)
}

// Parser for Maps: {key1 val1 key2 val2 ...} - WITHOUT outer ws
fn parse_map_internal(input: &str) -> IResult<&str, Value> {
    map(
        delimited(
            ws(char('{')), // Keep ws around opening delimiter
            separated_list0(ws_or_comment1, parse_map_entry_internal), // Use ws_or_comment1 as separator
            ws(char('}')), // Apply ws to closing delimiter
        ),
        |entries: Vec<(MapKey, Value)>| Value::Map(entries.into_iter().collect::<HashMap<_, _>>()),
    )(input)
}

// Define the actual recursive parser function
fn parse_value_internal(input: &str) -> IResult<&str, Value> {
    // NO ws here - handled by ws() calls in collection parsers or top-level parse_value
    alt((
        parse_list_internal, // Use internal versions
        parse_vector_internal,
        parse_map_internal,
        parse_nil,
        parse_bool,
        parse_float,
        parse_int,
        parse_string,
        parse_keyword,
        parse_symbol,
    ))(input)
}

// Top-level parser for a single value/expression, handles outer whitespace
pub fn parse_value(input: &str) -> IResult<&str, Value> {
    ws(parse_value_internal)(input) // Add ws wrapper here at the top level
}

// Top-level parser (for now, just parses a single value into an Expr)
pub fn parse_expr(input: &str) -> IResult<&str, Expr> {
    // Use the top-level parse_value which handles outer whitespace
    map(parse_value, Expr::Literal)(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::ToBigInt;
    use std::collections::HashMap;

    // Helper for float comparison, especially NaNs
    fn assert_float_eq(v1: &Value, v2: &Value) {
        match (v1, v2) {
            (Value::Float(f1), Value::Float(f2)) => {
                if f1.is_nan() && f2.is_nan() {
                    // NaNs are equal in this context
                } else {
                    assert_eq!(f1, f2);
                }
            }
            _ => panic!("Expected two Float values"),
        }
    }

    #[test]
    fn test_parse_nil() {
        assert_eq!(parse_nil("nil"), Ok(("", Value::Nil)));
        assert!(parse_nil("Nil").is_err());
        assert!(parse_nil("nil ").is_ok()); // Consumes only "nil"
    }

    #[test]
    fn test_parse_bool() {
        assert_eq!(parse_bool("true"), Ok(("", Value::Bool(true))));
        assert_eq!(parse_bool("false"), Ok(("", Value::Bool(false))));
        assert!(parse_bool("True").is_err());
        assert!(parse_bool("falsey").is_ok()); // Consumes only "false"
    }

    #[test]
    fn test_parse_int() {
        assert_eq!(
            parse_int("123"),
            Ok(("", Value::Int(123.to_bigint().unwrap())))
        );
        assert_eq!(
            parse_int("-45"),
            Ok(("", Value::Int((-45).to_bigint().unwrap())))
        );
        assert_eq!(parse_int("0"), Ok(("", Value::Int(0.to_bigint().unwrap()))));
        // Still consumes only digits, leaving trailing chars
        assert_eq!(
            parse_int("123a"),
            Ok(("a", Value::Int(123.to_bigint().unwrap())))
        );
        assert!(parse_int("abc").is_err());
        assert!(parse_int("- 123").is_err()); // '-' must be adjacent
                                              // These assertions should now pass
        assert!(parse_int("1.2").is_err());
        assert!(parse_int("1e5").is_err());
        assert!(parse_int("1.e5").is_err()); // Should fail because of '.'
    }

    #[test]
    fn test_parse_float() {
        assert_float_eq(&parse_float("1.23").unwrap().1, &Value::Float(1.23));
        assert_float_eq(&parse_float("-0.5").unwrap().1, &Value::Float(-0.5));
        assert_float_eq(&parse_float("1.").unwrap().1, &Value::Float(1.0)); // Ends with dot
        assert_float_eq(&parse_float(".5").unwrap().1, &Value::Float(0.5)); // Starts with dot
        assert_float_eq(&parse_float("1e5").unwrap().1, &Value::Float(1e5)); // Has exponent
        assert_float_eq(&parse_float("1.0e5").unwrap().1, &Value::Float(1.0e5)); // Dot and exponent
        assert_float_eq(&parse_float("1.2E-3").unwrap().1, &Value::Float(1.2e-3));
        assert_float_eq(&parse_float("-1.2e+3").unwrap().1, &Value::Float(-1.2e3));
        assert_float_eq(&parse_float("inf").unwrap().1, &Value::Float(f64::INFINITY));
        assert_float_eq(
            &parse_float("-inf").unwrap().1,
            &Value::Float(f64::NEG_INFINITY),
        );
        assert_float_eq(
            &parse_float("+inf").unwrap().1,
            &Value::Float(f64::INFINITY),
        );
        assert_float_eq(&parse_float("nan").unwrap().1, &Value::Float(f64::NAN));
        assert!(parse_float("1.23a").is_ok()); // Consumes only "1.23"
        assert!(parse_float(".").is_err());
        assert!(parse_float("e5").is_err());
        // Ensure it doesn't consume integers meant for parse_int
        assert!(parse_float("123").is_err()); // No dot or exponent
        assert!(parse_float("-45").is_err()); // No dot or exponent
    }

    #[test]
    fn test_parse_string() {
        // This should finally pass
        assert_eq!(
            parse_string(r#""""#),
            Ok(("", Value::String("".to_string())))
        );
        assert_eq!(
            parse_string(r#""hello""#),
            Ok(("", Value::String("hello".to_string())))
        );
        assert_eq!(
            parse_string(r#""hello world""#),
            Ok(("", Value::String("hello world".to_string())))
        );
        assert_eq!(
            parse_string(r#""with\"escapes\n\t\\""#),
            Ok(("", Value::String("with\"escapes\n\t\\".to_string())))
        );
        assert!(parse_string(r#""unterminated"#).is_err());
        assert!(parse_string(r#"no quotes"#).is_err());
        // This should still fail as '\e' is not a valid escape sequence here
        assert!(parse_string(r#""invalid\escape""#).is_err());
    }

    #[test]
    fn test_parse_symbol() {
        assert_eq!(
            parse_symbol("abc"),
            Ok(("", Value::Symbol("abc".to_string())))
        );
        assert_eq!(
            parse_symbol("a-b_c?"),
            Ok(("", Value::Symbol("a-b_c?".to_string())))
        );
        assert_eq!(parse_symbol("+"), Ok(("", Value::Symbol("+".to_string()))));
        assert_eq!(
            parse_symbol("set!"),
            Ok(("", Value::Symbol("set!".to_string())))
        );
        assert_eq!(
            parse_symbol("valid.symbol#1"),
            Ok(("", Value::Symbol("valid.symbol#1".to_string())))
        );
        assert_eq!(
            parse_symbol("a1"),
            Ok(("", Value::Symbol("a1".to_string())))
        );

        // Should not parse keywords or numbers
        assert!(parse_symbol("nil").is_err());
        assert!(parse_symbol("true").is_err());
        assert!(parse_symbol("false").is_err());
        assert!(parse_symbol("123").is_err());
        assert!(parse_symbol("-45").is_err());
        assert!(parse_symbol("1.23").is_err());
        assert!(parse_symbol(":keyword").is_err()); // Colon indicates keyword

        // Test partial consumption
        assert_eq!(
            parse_symbol("abc def"),
            Ok((" def", Value::Symbol("abc".to_string())))
        );
        assert_eq!(
            parse_symbol("abc)"),
            Ok((")", Value::Symbol("abc".to_string())))
        );

        // Invalid start characters
        assert!(parse_symbol(".dotstart").is_err());
        assert!(parse_symbol("#hashstart").is_err());
        assert!(parse_symbol(":colonstart").is_err());
    }

    #[test]
    fn test_parse_keyword() {
        assert_eq!(
            parse_keyword(":abc"),
            Ok(("", Value::Keyword("abc".to_string())))
        );
        assert_eq!(
            parse_keyword(":a-b_c?"),
            Ok(("", Value::Keyword("a-b_c?".to_string())))
        );
        assert_eq!(
            parse_keyword(":+"),
            Ok(("", Value::Keyword("+".to_string())))
        );
        assert_eq!(
            parse_keyword(":set!"),
            Ok(("", Value::Keyword("set!".to_string())))
        );
        assert_eq!(
            parse_keyword(":valid.keyword#1"),
            Ok(("", Value::Keyword("valid.keyword#1".to_string())))
        );
        assert_eq!(
            parse_keyword(":a1"), // Digit allowed inside
            Ok(("", Value::Keyword("a1".to_string())))
        );

        // Invalid keywords
        assert!(parse_keyword("abc").is_err()); // Missing colon
        assert!(parse_keyword(":").is_err()); // Colon only
                                              // This assertion should now pass
        assert!(parse_keyword(":123").is_err()); // Starts with digit after colon
        assert!(parse_keyword(":.abc").is_err()); // Starts with dot after colon

        // Test partial consumption
        assert_eq!(
            parse_keyword(":abc def"),
            Ok((" def", Value::Keyword("abc".to_string())))
        );
        assert_eq!(
            parse_keyword(":abc)"),
            Ok((")", Value::Keyword("abc".to_string())))
        );
    }

    #[test]
    fn test_parse_list() {
        // Use the public parse_value or parse_list_internal directly?
        // Tests should ideally use the public interface if possible.
        // Let's test parse_value with list input.
        assert_eq!(parse_value("()"), Ok(("", Value::List(vec![]))));
        assert_eq!(
            parse_value("(1 true \"hello\")"), // Test with parse_value
            Ok((
                "",
                Value::List(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Bool(true),
                    Value::String("hello".to_string()),
                ])
            ))
        );
        assert_eq!(
            parse_value("( 1 ( 2 3 ) nil )"), // Test with parse_value
            Ok((
                "", // parse_value consumes all surrounding whitespace
                Value::List(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::List(vec![
                        Value::Int(2.to_bigint().unwrap()),
                        Value::Int(3.to_bigint().unwrap()),
                    ]),
                    Value::Nil,
                ])
            ))
        );
        assert!(parse_value("(1 2").is_err()); // Unclosed
        assert!(parse_list_internal("[1 2]").is_err()); // Wrong brackets (parse_value tries parse_list first)
    }

    #[test]
    fn test_parse_vector() {
        assert_eq!(parse_value("[]"), Ok(("", Value::Vector(vec![]))));
        assert_eq!(
            parse_value("[ 1 true \"hello\" ]"), // Use parse_value
            Ok((
                "",
                Value::Vector(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Bool(true),
                    Value::String("hello".to_string()),
                ])
            ))
        );
        assert_eq!(
            parse_value("[ 1 [ 2 3 ] nil ]"),
            Ok((
                "",
                Value::Vector(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Vector(vec![
                        Value::Int(2.to_bigint().unwrap()),
                        Value::Int(3.to_bigint().unwrap()),
                    ]),
                    Value::Nil,
                ])
            ))
        );
        assert!(parse_value("[1 2").is_err()); // Unclosed
        assert!(parse_vector_internal("(1 2)").is_err()); // Wrong brackets
    }

    #[test]
    fn test_parse_map() {
        assert_eq!(parse_value("{}"), Ok(("", Value::Map(HashMap::new()))));

        let mut expected_map1 = HashMap::new();
        expected_map1.insert(
            MapKey::Keyword("a".to_string()),
            Value::Int(1.to_bigint().unwrap()),
        );
        expected_map1.insert(MapKey::String("b".to_string()), Value::Bool(true));

        assert_eq!(
            parse_value("{ :a 1 \"b\" true }"), // Use parse_value
            Ok(("", Value::Map(expected_map1)))
        );

        let mut expected_map2 = HashMap::new();
        let mut inner_map = HashMap::new();
        inner_map.insert(MapKey::Int(10.to_bigint().unwrap()), Value::Nil);
        expected_map2.insert(MapKey::Symbol("outer".to_string()), Value::Map(inner_map));

        assert_eq!(
            parse_value("{ outer { 10 nil } }"),
            Ok(("", Value::Map(expected_map2)))
        );

        // Test invalid key type
        assert!(parse_value("{ [1 2] 3 }").is_err()); // Vector cannot be a key
        assert!(parse_value("{ 1.5 true }").is_err()); // Float cannot be a key

        // Test uneven number of elements
        assert!(parse_value("{ :a 1 :b }").is_err());
        assert!(parse_value("{ :a }").is_err());
    }

    #[test]
    fn test_parse_value_literals() {
        assert_eq!(parse_value("  nil  "), Ok(("", Value::Nil)));
        assert_eq!(parse_value("true"), Ok(("", Value::Bool(true))));
        assert_eq!(
            parse_value("\t -99 \n"),
            Ok(("", Value::Int((-99).to_bigint().unwrap())))
        );
        assert_float_eq(&parse_value(" 1.23 ").unwrap().1, &Value::Float(1.23));
        assert_float_eq(&parse_value(" nan ").unwrap().1, &Value::Float(f64::NAN));
        assert_eq!(
            parse_value(r#" "hello \n world" "#),
            Ok(("", Value::String("hello \n world".to_string())))
        );
        // Test order sensitivity
        assert_eq!(
            parse_value("123"), // Should be Int
            Ok(("", Value::Int(123.to_bigint().unwrap())))
        );
        assert_float_eq(&parse_value("123.0").unwrap().1, &Value::Float(123.0)); // Should be Float
        assert_float_eq(&parse_value("1e3").unwrap().1, &Value::Float(1000.0)); // Should be Float
        assert_eq!(
            parse_value("  my-symbol  "),
            Ok(("", Value::Symbol("my-symbol".to_string())))
        );
        assert_eq!(
            parse_value(" :a-keyword? "),
            Ok(("", Value::Keyword("a-keyword?".to_string())))
        );

        // Test order sensitivity again
        assert_eq!(parse_value("nil"), Ok(("", Value::Nil))); // Not a symbol
        assert_eq!(parse_value("true"), Ok(("", Value::Bool(true)))); // Not a symbol
        assert_eq!(
            parse_value("123"),
            Ok(("", Value::Int(123.to_bigint().unwrap())))
        ); // Not a symbol
        assert_float_eq(&parse_value("1.0").unwrap().1, &Value::Float(1.0)); // Not a symbol
    }

    #[test]
    fn test_parse_value_collections() {
        assert_eq!(parse_value("() "), Ok(("", Value::List(vec![]))));
        // This assertion should now pass as parse_float won't consume "1"
        assert_eq!(
            parse_value(" [ 1 ] "),
            Ok(("", Value::Vector(vec![Value::Int(1.to_bigint().unwrap())])))
        );
        assert_eq!(parse_value(" { } "), Ok(("", Value::Map(HashMap::new()))));
        assert_eq!(
            parse_value("( [ :a ] { } ) "),
            Ok((
                "",
                Value::List(vec![
                    Value::Vector(vec![Value::Keyword("a".to_string())]),
                    Value::Map(HashMap::new()),
                ])
            ))
        );
        // Add a test with floats inside collections
        // Using helper method to extract value for comparison
        let parsed_vec = parse_value("[ 1.5 ]").unwrap().1;
        assert_float_eq(&parsed_vec.get_vector().unwrap()[0], &Value::Float(1.5));
    }

    #[test]
    fn test_parse_expr_literal() {
        assert_eq!(
            parse_expr(" 12345 "),
            Ok(("", Expr::Literal(Value::Int(12345.to_bigint().unwrap()))))
        );
        assert_eq!(
            parse_expr(" false"),
            Ok(("", Expr::Literal(Value::Bool(false))))
        );
        // Using if let to handle the result and avoid unreachable pattern warning
        let parse_result = parse_expr(" -inf ");
        assert!(parse_result.is_ok(), "Parsing '-inf' failed");
        if let Ok((_, parsed_expr)) = parse_result {
            let expected_value = Value::Float(f64::NEG_INFINITY);
            match parsed_expr {
                Expr::Literal(ref actual_value) => assert_float_eq(actual_value, &expected_value),
                // This branch might be unreachable if parse_expr only returns Expr::Literal
                #[allow(unreachable_patterns)]
                _ => panic!("Expected Expr::Literal, got {:?}", parsed_expr),
            }
        }
        assert_eq!(
            parse_expr(r#" "string" "#),
            Ok(("", Expr::Literal(Value::String("string".to_string()))))
        );
        assert_eq!(
            parse_expr(" some_symbol "),
            Ok(("", Expr::Literal(Value::Symbol("some_symbol".to_string()))))
        );
        assert_eq!(
            parse_expr(" :keyword123 "),
            Ok(("", Expr::Literal(Value::Keyword("keyword123".to_string()))))
        );
        assert_eq!(
            parse_expr(" (1 2) "),
            Ok((
                "",
                Expr::Literal(Value::List(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Int(2.to_bigint().unwrap())
                ]))
            ))
        );
        assert_eq!(
            parse_expr(" [ :a ] "),
            Ok((
                "",
                Expr::Literal(Value::Vector(vec![Value::Keyword("a".to_string())]))
            ))
        );
        let mut expected_map = HashMap::new();
        expected_map.insert(
            MapKey::String("key".to_string()),
            Value::Int(100.to_bigint().unwrap()),
        );
        assert_eq!(
            parse_expr(" { \"key\" 100 } "),
            Ok(("", Expr::Literal(Value::Map(expected_map))))
        );
    }

    // New tests for parsing comments
    #[test]
    fn test_parse_with_comments() {
        // Comments before value
        assert_eq!(
            parse_value("; this is a comment\n 123"),
            Ok(("", Value::Int(123.to_bigint().unwrap())))
        );
        // Comments after value
        assert_eq!(
            parse_value("true ; another comment"),
            Ok(("", Value::Bool(true)))
        );
        // Comments surrounding value
        assert_eq!(
            parse_value("; comment before\n nil ; comment after"),
            Ok(("", Value::Nil))
        );
        // Comments inside list
        assert_eq!(
            parse_value("(1 ; item 1\n 2 ; item 2\n )"),
            Ok((
                "",
                Value::List(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Int(2.to_bigint().unwrap()),
                ])
            ))
        );
        // Comments inside vector
        assert_eq!(
            parse_value("[ :a ; keyword a \n :b ; keyword b \n ]"),
            Ok((
                "",
                Value::Vector(vec![
                    Value::Keyword("a".to_string()),
                    Value::Keyword("b".to_string()),
                ])
            ))
        );

        assert!(parse_value("[ :a ; keyword a \n :b ; keyword b ]").is_err());
        // Comments inside map
        let mut expected_map = HashMap::new();
        expected_map.insert(
            MapKey::String("key".to_string()),
            Value::Int(1.to_bigint().unwrap()),
        );
        expected_map.insert(MapKey::Keyword("another".to_string()), Value::Bool(false));
        assert_eq!(
            parse_value("{ \"key\" ; map key \n 1 ; map value \n :another false ; next pair \n }"),
            Ok(("", Value::Map(expected_map)))
        );
        // Comment within a string (should be part of the string)
        assert_eq!(
            parse_value(r#""a string ; with a comment inside""#),
            Ok((
                "",
                Value::String("a string ; with a comment inside".to_string())
            ))
        );
        // Multiple comments and whitespace
        assert_eq!(
            parse_value("  ; comment 1 \n   ( ; comment 2 \n 1 \n ; comment 3 \n ) ; comment 4 "),
            Ok(("", Value::List(vec![Value::Int(1.to_bigint().unwrap())])))
        );
        // Empty input with comments
        assert!(parse_value("; only comment").is_err()); // Expecting a value, not just comment
        assert!(parse_value(" ; ; ; ").is_err()); // Expecting a value

        // Test with parse_expr
        assert_eq!(
            parse_expr("; comment before expr\n 42"),
            Ok(("", Expr::Literal(Value::Int(42.to_bigint().unwrap()))))
        );
    }

    #[test]
    fn test_parse_comment_only_input() {
        // Test cases where the input contains only comments or whitespace and comments
        let result = parse_value("; just a comment");
        assert!(
            result.is_err(),
            "Parsing only a comment should fail when expecting a value"
        );

        let result = parse_value("  ; comment with leading space \n ; another comment ");
        assert!(
            result.is_err(),
            "Parsing only comments and whitespace should fail"
        );

        // Ensure parse_expr also handles this correctly
        let result_expr = parse_expr("; comment for expr");
        assert!(
            result_expr.is_err(),
            "parse_expr should fail on comment-only input"
        );
    }

    #[test]
    fn test_comment_at_eof() {
        // Test parsing a value followed immediately by EOF after a comment
        assert_eq!(
            parse_value("123 ; comment at end"),
            Ok(("", Value::Int(123.to_bigint().unwrap())))
        );
        assert_eq!(
            parse_value("(1 2) ; list comment"),
            Ok((
                "",
                Value::List(vec![
                    Value::Int(1.to_bigint().unwrap()),
                    Value::Int(2.to_bigint().unwrap())
                ])
            ))
        );
    }
}

// Helper method added to Value for testing convenience (optional)
impl Value {
    fn get_vector(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Vector(v) => Some(v),
            _ => None,
        }
    }
}
