use neon::search::parse_results;
#[test]
fn parses_ddg_results_and_unwraps_redirects() {
    let html = r#"<div class="results"><div class="result"><div class="result__body"><h2><a class="result__a" href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fguide">Rust guide</a></h2><a class="result__snippet">A useful reference</a></div></div><div class="result"><a class="result__a" href="https://example.net">Second result</a></div></div>"#;
    let rows = parse_results(html, 1).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Rust guide");
    assert!(rows[0].url.starts_with("https://example.org/guide"));
    assert_eq!(rows[0].snippet.as_deref(), Some("A useful reference"));
}
#[test]
fn refuses_invalid_result_limit() {
    assert!(parse_results("", 0).is_err());
}
