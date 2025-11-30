use ccos::examples_common::discovery_utils::{
    derive_server_name_from_repo_url, extract_suggestion_from_text,
};

#[test]
fn test_derive_server_name_from_repo_url_ok() {
    let url = "https://github.com/octocat/Hello-World";
    let got = derive_server_name_from_repo_url(url);
    assert_eq!(got, Some("octocat/Hello-World".to_string()));
}

#[test]
fn test_derive_server_name_from_repo_url_trailing_slash() {
    let url = "https://github.com/octocat/Hello-World/";
    let got = derive_server_name_from_repo_url(url);
    assert_eq!(got, Some("octocat/Hello-World".to_string()));
}

#[test]
fn test_extract_suggestion_json() {
    let text = "{".to_string() + "\"repo\": \"hello-world\"}";
    let got = extract_suggestion_from_text(&text, "repo");
    assert_eq!(got, Some("hello-world".to_string()));
}

#[test]
fn test_extract_suggestion_colon() {
    let text = "repo: hello-world";
    let got = extract_suggestion_from_text(text, "repo");
    assert_eq!(got, Some("hello-world".to_string()));
}

#[test]
fn test_extract_suggestion_owner_repo() {
    let text = "octocat/Hello-World";
    let got = extract_suggestion_from_text(text, "repo");
    assert_eq!(got, Some("Hello-World".to_string()));
}

#[test]
fn test_extract_suggestion_bare_token() {
    let text = "Dummy";
    let got = extract_suggestion_from_text(text, "repo");
    assert_eq!(got, Some("Dummy".to_string()));
}
