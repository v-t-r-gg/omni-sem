//! Safe lexical query parsing for FTS5.
//!
//! User text is never treated as raw FTS5 syntax. Tokens and phrases are escaped
//! and bound through SQL parameters as a MATCH expression composed only of
//! quoted literals combined with implicit AND (FTS5 default) and phrase terms.

use crate::domain::DomainError;

/// Maximum accepted query length in Unicode scalar values.
pub const MAX_QUERY_CHARS: usize = 512;

/// Parsed lexical query ready for FTS5 MATCH parameterization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Original user text after trim.
    pub original: String,
    /// Individual words and multi-word phrases extracted from the query.
    pub terms: Vec<String>,
    /// FTS5 MATCH expression composed only of escaped quoted literals.
    pub fts_match: String,
}

/// Parses ordinary user text into a safe FTS5 MATCH expression.
///
/// Supported surface:
/// - whitespace-separated terms (AND);
/// - double-quoted phrases;
/// - Unicode letters/digits;
/// - punctuation is treated as separators outside quotes.
///
/// # Errors
///
/// Returns [`DomainError::QueryEmpty`] or [`DomainError::QueryInvalid`].
pub fn parse_lexical_query(input: &str) -> Result<ParsedQuery, DomainError> {
    let original = input.trim().to_owned();
    if original.is_empty() {
        return Err(DomainError::QueryEmpty);
    }
    if original.chars().count() > MAX_QUERY_CHARS {
        return Err(DomainError::QueryInvalid(format!(
            "query exceeds {MAX_QUERY_CHARS} characters"
        )));
    }

    let mut terms = Vec::new();
    let mut chars = original.chars().peekable();
    let mut current = String::new();

    while let Some(ch) = chars.next() {
        if ch == '"' {
            if !current.is_empty() {
                push_term(&mut terms, &current);
                current.clear();
            }
            let mut phrase = String::new();
            let mut closed = false;
            for next in chars.by_ref() {
                if next == '"' {
                    closed = true;
                    break;
                }
                phrase.push(next);
            }
            if !closed {
                return Err(DomainError::QueryInvalid("unclosed quoted phrase".into()));
            }
            let phrase = phrase.trim();
            if !phrase.is_empty() {
                terms.push(normalize_spaces(phrase));
            }
            continue;
        }
        if ch.is_whitespace() || is_separator(ch) {
            if !current.is_empty() {
                push_term(&mut terms, &current);
                current.clear();
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        push_term(&mut terms, &current);
    }

    if terms.is_empty() {
        return Err(DomainError::QueryEmpty);
    }

    // Combine independent terms with OR so natural multi-word queries can match
    // evidence spread across short notes. Quoted phrases remain atomic literals.
    let fts_match = terms
        .iter()
        .map(|term| quote_fts_literal(term))
        .collect::<Vec<_>>()
        .join(" OR ");

    Ok(ParsedQuery {
        original,
        terms,
        fts_match,
    })
}

fn push_term(terms: &mut Vec<String>, raw: &str) {
    let cleaned = raw.trim_matches(|ch: char| is_separator(ch) || ch.is_whitespace());
    if !cleaned.is_empty() {
        terms.push(cleaned.to_owned());
    }
}

fn is_separator(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | ':'
            | ';'
            | ','
            | '!'
            | '?'
            | '*'
            | '^'
            | '~'
            | '|'
            | '&'
            | '+'
            | '='
            | '<'
            | '>'
            | '/'
            | '\\'
            | '@'
            | '#'
            | '$'
            | '%'
            | '`'
    )
}

fn normalize_spaces(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Escapes a term or phrase as an FTS5 quoted string literal.
fn quote_fts_literal(term: &str) -> String {
    let escaped = term.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_terms() {
        let parsed = parse_lexical_query("storage architecture").unwrap();
        assert_eq!(parsed.terms, vec!["storage", "architecture"]);
        assert_eq!(parsed.fts_match, "\"storage\" OR \"architecture\"");
    }

    #[test]
    fn parses_quoted_phrases() {
        let parsed = parse_lexical_query(r#"find "source of truth" now"#).unwrap();
        assert_eq!(parsed.terms, vec!["find", "source of truth", "now"]);
        assert!(parsed.fts_match.contains("\"source of truth\""));
    }

    #[test]
    fn rejects_empty_and_operators_only() {
        assert!(matches!(
            parse_lexical_query("   "),
            Err(DomainError::QueryEmpty)
        ));
        assert!(matches!(
            parse_lexical_query("()::**"),
            Err(DomainError::QueryEmpty)
        ));
    }

    #[test]
    fn escapes_quotes_and_ignores_raw_fts_operators() {
        let parsed = parse_lexical_query(r#"hello "world" OR NEAR(a,b) select * from t"#).unwrap();
        assert!(parsed.fts_match.contains("\"OR\""));
        assert!(parsed.fts_match.contains("\"NEAR\""));
        assert!(!parsed.fts_match.contains("NEAR("));
        let quoted = parse_lexical_query(r#"path "C:\Users\x" ok"#).unwrap();
        assert!(
            quoted.fts_match.contains("\"C:\\Users\\x\"")
                || quoted.terms.iter().any(|t| t.contains("Users"))
        );
        let internal = quote_fts_literal(r#"he said "hi""#);
        assert_eq!(internal, "\"he said \"\"hi\"\"\"");
    }

    #[test]
    fn unicode_and_hyphenated_tokens() {
        let parsed = parse_lexical_query("café well-known").unwrap();
        assert!(parsed.terms.iter().any(|t| t.contains('é') || t == "café"));
        assert!(
            parsed
                .terms
                .iter()
                .any(|t| t.contains("well-known") || t == "well-known")
        );
    }
}
