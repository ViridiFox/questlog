// Cursor-aware completion for the reset rule input field.
//
// Completion is derived from the context around the cursor position, not the
// entire field value.  The returned `Completion` carries the byte range that
// should be replaced, a list of context-valid `candidates` (primary), and a
// list of full-skeleton `examples` (secondary / dimmed in the popup).
//
// Cursor context → candidates / examples:
//   `da|`                              top-level bare token
//     candidates: `daily`              (filtered by prefix)
//     examples:   all non-prefix-filtered skeletons
//   `{ type = "int|" }`               quoted value for `type`
//     candidates: `interval`, …
//   `{ type = "weekly", d| }`         key name after comma
//     candidates: `day`, `time`
//   `{ type = "weekly", day = "mo|" }` quoted weekday value
//     candidates: `monday`, …
//   `[|`                              array opener
//     examples:   array skeleton templates

// ── public API ────────────────────────────────────────────────────────────────

/// Result of a completion query.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Completion {
    /// Byte offset in the original input where the replacement begins.
    /// Used for `candidates`.
    pub replace_start: usize,
    /// Byte offset where the replacement ends (exclusive).  Used for
    /// `candidates`.  After accepting, the cursor is placed at
    /// `replace_start + candidate.len()`.
    pub replace_end: usize,
    /// Context-valid completions at the cursor position (e.g. key names,
    /// value enum members).  These are shown first in the popup.
    /// When these are string-value completions the surrounding quotes are NOT
    /// included — the splice logic keeps the opening `"` and adds a closing `"`
    /// when needed.
    pub candidates: Vec<String>,
    /// Full skeleton examples for the rule as a whole (e.g. `{ type =
    /// "interval", hours = 4 }`).  Shown below candidates in the popup with
    /// dimmed styling.
    pub examples: Vec<String>,
    /// Override replace range for `examples`.  When `None`, examples use the
    /// same `replace_start`/`replace_end` as candidates.
    pub example_replace_range: Option<(usize, usize)>,
    /// True when the candidates are *value* completions (the token lives inside
    /// `"…"`) so the surrounding quotes must be kept by the splice.
    pub in_quoted_value: bool,
}

impl Completion {
    fn empty() -> Self {
        Self::default()
    }

    /// True when there is nothing to show in the completion popup.
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty() && self.examples.is_empty()
    }

    /// Total number of items (candidates + examples).
    pub fn total_len(&self) -> usize {
        self.candidates.len() + self.examples.len()
    }

    /// Return the item at `index` spanning candidates then examples, or `None`.
    pub fn get(&self, index: usize) -> Option<&str> {
        if index < self.candidates.len() {
            self.candidates.get(index).map(String::as_str)
        } else {
            self.examples.get(index - self.candidates.len()).map(String::as_str)
        }
    }

    /// Return the effective `(replace_start, replace_end)` for the item at
    /// `index`.  Candidates always use the primary range; examples fall back to
    /// `example_replace_range` if set, otherwise the primary range.
    pub fn replace_range_for(&self, index: usize) -> (usize, usize) {
        if index < self.candidates.len() {
            (self.replace_start, self.replace_end)
        } else if let Some((s, e)) = self.example_replace_range {
            (s, e)
        } else {
            (self.replace_start, self.replace_end)
        }
    }
}

/// Compute cursor-aware completions for `input` at byte offset `cursor`.
pub fn reset_completions(input: &str, cursor: usize) -> Completion {
    let cursor = cursor.min(input.len());
    let before = &input[..cursor];
    let trimmed_before = before.trim_start();

    // ── array context ────────────────────────────────────────────────────────
    if trimmed_before.starts_with('[') {
        // If the cursor is inside an object within the array (past a `{`),
        // delegate to table completion for that object.
        if trimmed_before.contains('{') {
            return table_completions_at(input, cursor);
        }
        return array_completions(input, cursor);
    }

    // ── inline table context ─────────────────────────────────────────────────
    if trimmed_before.starts_with('{') {
        return table_completions_at(input, cursor);
    }

    // ── top-level bare token ─────────────────────────────────────────────────
    top_level_completions(input, cursor)
}

// ── top-level (bare shorthand or initial character) ───────────────────────────

fn top_level_completions(input: &str, cursor: usize) -> Completion {
    let before = &input[..cursor];
    // The token is the sequence of non-whitespace chars leading up to the cursor.
    let token_start = before
        .rfind(|c: char| c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    let token = &before[token_start..];

    // The replacement ends at the cursor (we don't consume trailing chars).
    let replace_end = cursor;

    // Primary: bare shorthands that are valid as-is.
    let shorthands: &[&str] = &["daily", "weekly"];
    let candidates: Vec<String> = shorthands
        .iter()
        .filter(|s| s.starts_with(token))
        .map(|s| s.to_string())
        .collect();

    // Secondary: full skeleton templates (shown dimmed as examples).
    let skeleton_templates: &[&str] = &[
        r#"{ type = "daily" }"#,
        r#"{ type = "daily", time = "00:00" }"#,
        r#"{ type = "weekly" }"#,
        r#"{ type = "weekly", day = "monday", time = "00:00" }"#,
        r#"{ type = "interval", hours = 4 }"#,
        r#"{ type = "interval", minutes = 30 }"#,
        r#"{ type = "interval", days = 1 }"#,
        r#"{ type = "schedule", hours = 2 }"#,
        r#"{ type = "schedule", minutes = 15 }"#,
        "[ ]",
    ];
    let examples: Vec<String> = skeleton_templates
        .iter()
        .filter(|s| s.starts_with(token))
        .map(|s| s.to_string())
        .collect();

    Completion {
        replace_start: token_start,
        replace_end,
        candidates,
        examples,
        example_replace_range: None,
        in_quoted_value: false,
    }
}

// ── array context ─────────────────────────────────────────────────────────────

fn array_completions(input: &str, cursor: usize) -> Completion {
    let examples: Vec<String> = vec![
        r#"[{ type = "daily", time = "08:00" }, { type = "daily", time = "20:00" }]"#.to_string(),
        r#"[{ type = "daily" }, { type = "weekly" }]"#.to_string(),
    ];
    // Replace everything from the `[` up to the end of the input.
    let start = input[..cursor].rfind('[').unwrap_or(0);
    Completion {
        replace_start: start,
        replace_end: input.len(),
        candidates: vec![],
        examples,
        example_replace_range: None,
        in_quoted_value: false,
    }
}

// ── inline table context ──────────────────────────────────────────────────────

/// Find the byte offset (in `s`) just past the `}` that closes the `{` at
/// `s[0]`.  Returns `s.len()` if unclosed.  Handles nested braces and
/// string literals.
fn find_object_end(s: &str) -> usize {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + c.len_utf8();
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    s.len()
}

/// Find the byte offset of the `{` that opens the innermost inline table
/// containing the cursor.  Returns `None` if the cursor is not inside any
/// `{…}` block.
fn innermost_brace_start(before: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut last_open: Option<usize> = None;
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in before.char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => {
                    depth += 1;
                    last_open = Some(i);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        last_open = None;
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    if depth > 0 { last_open } else { None }
}

/// Returns the cursor context inside a `{ … }` inline table and derives
/// appropriate completions.  Only the object that directly contains the cursor
/// is inspected — not sibling or parent objects.
fn table_completions_at(input: &str, cursor: usize) -> Completion {
    let before = &input[..cursor];

    // Narrow down to the innermost object containing the cursor so that
    // `extract_type` / `extract_keys` only see the relevant object's fields.
    let obj_start = innermost_brace_start(before).unwrap_or(0);
    // The object slice runs from `obj_start` to the end of `input`.
    let obj_slice = &input[obj_start..];

    let rule_type = extract_type(obj_slice);
    let existing_keys = extract_keys(obj_slice);

    // Determine what the cursor is sitting on/in.
    let ctx = cursor_context(before);

    match ctx {
        CursorCtx::InQuotedValue { key, value_start, value_prefix } => {
            // We're inside a quoted value.  Offer valid values for that key.
            let (candidates, hint) = value_candidates(&key, &value_prefix);
            let examples = hint
                .filter(|_| value_prefix.is_empty())
                .map(|h| h.to_string())
                .into_iter()
                .collect();
            Completion {
                replace_start: value_start,
                replace_end: cursor,
                candidates,
                examples,
                example_replace_range: None,
                in_quoted_value: true,
            }
        }

        CursorCtx::InKeyName { key_start, key_prefix } => {
            // We're typing a key name.  Offer missing keys valid for the
            // current rule type.
            let candidates = key_candidates(rule_type.as_deref(), &existing_keys, &key_prefix);
            Completion {
                replace_start: key_start,
                replace_end: cursor,
                candidates,
                examples: vec![],
                example_replace_range: None,
                in_quoted_value: false,
            }
        }

        CursorCtx::AfterEquals { key } => {
            // Cursor is right after `key =` with no quote started yet.
            // Enumerable string values → quoted candidates.
            // Free-form string values → non-insertable hint in examples.
            // Numeric values → non-insertable `<N>` hint in examples.
            let (candidates, examples) = match key.as_str() {
                "type" => {
                    let (vals, _) = value_candidates("type", "");
                    let quoted = vals.into_iter().map(|v| format!(r#""{v}""#)).collect();
                    (quoted, vec![])
                }
                "day" => {
                    let (vals, _) = value_candidates("day", "");
                    let quoted = vals.into_iter().map(|v| format!(r#""{v}""#)).collect();
                    (quoted, vec![])
                }
                "time" => (vec![], vec!["\"<HH:MM>\"".to_string()]),
                "anchor" => (vec![], vec!["\"<YYYY-MM-DDTHH:MM:SSZ>\"".to_string()]),
                "minutes" | "hours" | "days" | "weeks" => (vec![], vec!["<N>".to_string()]),
                _ => (vec![], vec![]),
            };
            // Insert at cursor; nothing to replace.
            Completion {
                replace_start: cursor,
                replace_end: cursor,
                candidates,
                examples,
                example_replace_range: None,
                in_quoted_value: false,
            }
        }

        CursorCtx::AfterOpenBrace | CursorCtx::AfterComma => {
            // Cursor is right after `{` or `,` (possibly with whitespace).
            if rule_type.is_none() {
                // No type yet: primary candidate is the `type` key; skeleton
                // templates are shown as secondary examples.
                let skeletons: Vec<String> = vec![
                    r#"{ type = "daily" }"#.to_string(),
                    r#"{ type = "daily", time = "00:00" }"#.to_string(),
                    r#"{ type = "weekly" }"#.to_string(),
                    r#"{ type = "weekly", day = "monday", time = "00:00" }"#.to_string(),
                    r#"{ type = "interval", hours = 4 }"#.to_string(),
                    r#"{ type = "interval", minutes = 30 }"#.to_string(),
                    r#"{ type = "schedule", hours = 2 }"#.to_string(),
                    r#"{ type = "schedule", minutes = 15 }"#.to_string(),
                ];
                // The `type` candidate inserts at the cursor position; examples
                // replace the current object from its opening `{` to its closing
                // `}` (or end of object if unclosed).
                let obj_end = obj_start + find_object_end(obj_slice);
                Completion {
                    replace_start: cursor,
                    replace_end: cursor,
                    candidates: vec!["type".to_string()],
                    examples: skeletons,
                    example_replace_range: Some((obj_start, obj_end)),
                    in_quoted_value: false,
                }
            } else {
                // Type is known — offer missing keys (no prefix filter).
                let candidates = key_candidates(rule_type.as_deref(), &existing_keys, "");
                // Replace starting at the current cursor (nothing to delete).
                Completion {
                    replace_start: cursor,
                    replace_end: cursor,
                    candidates,
                    examples: vec![],
                    example_replace_range: None,
                    in_quoted_value: false,
                }
            }
        }

        CursorCtx::Other => Completion::empty(),
    }
}

// ── cursor context classification ─────────────────────────────────────────────

#[derive(Debug)]
enum CursorCtx {
    /// Cursor is inside a quoted value: `key = "prefix|`
    InQuotedValue {
        key: String,
        /// Byte offset of the first char after the opening `"`.
        value_start: usize,
        /// The text typed so far inside the quotes (up to the cursor).
        value_prefix: String,
    },
    /// Cursor is on a key name token (before `=`).
    InKeyName {
        /// Byte offset of the first char of the key token.
        key_start: usize,
        /// Partial key name up to the cursor.
        key_prefix: String,
    },
    /// Cursor is directly after `=` (and optional whitespace) with no opening
    /// quote yet: `key = |`.  The key name is included so value hints can be
    /// offered.
    AfterEquals { key: String },
    /// Cursor is directly after `{` (ignoring whitespace).
    AfterOpenBrace,
    /// Cursor is directly after `,` (ignoring whitespace).
    AfterComma,
    /// Any other position (inside a numeric value, …).
    Other,
}

/// Classify where the cursor sits in the `before` slice (text before the cursor).
fn cursor_context(before: &str) -> CursorCtx {
    let b = before.trim_end();

    // ── inside a quoted value? ────────────────────────────────────────────────
    // Find the last unmatched `"` in `before`.  If there is one, the cursor is
    // inside a quoted value.  Track what key it belongs to by scanning backwards
    // for `<key> =` before that quote.
    if let Some(open_quote) = last_open_quote_pos(before) {
        let value_start = open_quote + 1; // byte after the `"`
        let value_prefix = before[value_start..].to_string();
        // The key is the identifier immediately before the `=` sign that
        // precedes this quote.
        let before_quote = &before[..open_quote];
        let key = extract_last_key(before_quote).unwrap_or_default();
        return CursorCtx::InQuotedValue {
            key,
            value_start,
            value_prefix,
        };
    }

    // ── after `{` (with optional whitespace)? ────────────────────────────────
    if b.ends_with('{') {
        return CursorCtx::AfterOpenBrace;
    }

    // ── after `,` (with optional whitespace)? ────────────────────────────────
    if b.ends_with(',') {
        return CursorCtx::AfterComma;
    }

    // ── directly after `=` (no quote yet)? ───────────────────────────────────
    if b.ends_with('=') {
        let key = extract_last_key(b).unwrap_or_default();
        return CursorCtx::AfterEquals { key };
    }

    // ── inside a key name token? ──────────────────────────────────────────────
    // Key names are simple identifiers (ASCII alphanumeric + `_`), appearing
    // after `{` or `,` and before `=`.
    // Detect: the text after the last `{` or `,` (up to cursor) matches
    // an identifier that hasn't been followed by `=` yet.
    let after_sep = after_last_separator(before);
    let trimmed = after_sep.trim_start();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        let key_start = before.len() - after_sep.len() + (after_sep.len() - trimmed.len());
        return CursorCtx::InKeyName {
            key_start,
            key_prefix: trimmed.to_string(),
        };
    }

    CursorCtx::Other
}

/// Find the byte position of the last unmatched `"` in `s`.
/// Returns `None` if all quotes are paired.
fn last_open_quote_pos(s: &str) -> Option<usize> {
    let mut last_unmatched: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c == '"' {
            if last_unmatched.is_some() {
                last_unmatched = None; // close the open quote
            } else {
                last_unmatched = Some(i);
            }
        }
    }
    last_unmatched
}

/// Return the slice of `before` that follows the last `{` or `,`.
fn after_last_separator(before: &str) -> &str {
    let sep = before
        .rfind(['{', ','])
        .map(|i| i + 1)
        .unwrap_or(0);
    &before[sep..]
}

/// Extract the key name from the tail end of `s` — the last bare identifier
/// that is immediately followed by optional whitespace and then `=`.
fn extract_last_key(s: &str) -> Option<String> {
    // Walk backwards to find `= <ws> key_end`, then collect the identifier.
    let trimmed = s.trim_end();
    // The text should end with something like `key` (after which `= "` follows).
    // But we're looking for the key that owns the upcoming `=`.
    // Find the last `=` in `s`.
    let eq_pos = trimmed.rfind('=')?;
    let before_eq = trimmed[..eq_pos].trim_end();
    // Collect the trailing identifier.
    let key_end = before_eq.len();
    let key_start = before_eq
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    if key_start < key_end {
        Some(before_eq[key_start..key_end].to_string())
    } else {
        None
    }
}

// ── candidate helpers ─────────────────────────────────────────────────────────

/// Valid completions for a string value associated with `key`.
/// Returns `(candidates, hint)` where `hint` is a non-insertable format hint
/// shown when there are no enumerable candidates (e.g. free-form fields).
fn value_candidates(key: &str, prefix: &str) -> (Vec<String>, Option<&'static str>) {
    match key {
        "type" => {
            let all = &["daily", "weekly", "interval", "schedule"];
            let candidates = all
                .iter()
                .filter(|s| s.starts_with(prefix))
                .map(|s| s.to_string())
                .collect();
            (candidates, None)
        }
        "day" => {
            let all = &[
                "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
            ];
            let candidates = all
                .iter()
                .filter(|s| s.starts_with(prefix))
                .map(|s| s.to_string())
                .collect();
            (candidates, None)
        }
        // Free-form fields: no enumerable candidates, only a format hint.
        "time" => (vec![], Some("<HH:MM>")),
        "anchor" => (vec![], Some("<YYYY-MM-DDTHH:MM:SSZ>")),
        _ => (vec![], None),
    }
}

/// Valid key-name completions for the given rule type, excluding already-present
/// keys, filtered by `prefix`.
fn key_candidates(rule_type: Option<&str>, existing_keys: &[String], prefix: &str) -> Vec<String> {
    let all_keys: &[&str] = match rule_type {
        None | Some("") => &["type"],
        Some("daily") => &["type", "time"],
        Some("weekly") => &["type", "day", "time"],
        Some("interval") => &["type", "minutes", "hours", "days", "weeks"],
        Some("schedule") => &["type", "minutes", "hours", "days", "weeks", "anchor"],
        Some(_) => &[],
    };
    all_keys
        .iter()
        .filter(|k| !existing_keys.iter().any(|e| e == *k))
        .filter(|k| k.starts_with(prefix))
        .map(|s| s.to_string())
        .collect()
}

// ── shared helpers ────────────────────────────────────────────────────────────

/// Extract the value of `type = "..."` from an inline table string, if present.
pub fn extract_type(input: &str) -> Option<String> {
    let after_type = input.find("type")?.checked_add(4)?;
    let rest = input[after_type..].trim_start();
    let rest = rest.strip_prefix('=')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Return all key names already present in the inline table.
pub fn extract_keys(input: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !c.is_ascii_alphabetic() && c != '_' {
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, nc)) = chars.peek() {
            if nc.is_ascii_alphanumeric() || nc == '_' {
                end = j + nc.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let word = &input[i..end];
        let after = input[end..].trim_start();
        if after.starts_with('=') && !after.starts_with("==") {
            keys.push(word.to_string());
        }
    }
    keys
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // helper: cursor at end of string
    fn compl(s: &str) -> Completion {
        reset_completions(s, s.len())
    }

    #[test]
    fn empty_returns_all_top_level() {
        let c = compl("");
        assert!(c.candidates.iter().any(|s| s == "daily"));
        assert!(c.candidates.iter().any(|s| s == "weekly"));
        // Skeleton templates are secondary examples, not primary candidates.
        assert!(c.examples.iter().any(|s| s.starts_with("{ type")));
    }

    #[test]
    fn prefix_filters_top_level() {
        let c = compl("da");
        assert!(c.candidates.contains(&"daily".to_string()));
        assert!(!c.candidates.contains(&"weekly".to_string()));
        assert_eq!(c.replace_start, 0);
        assert_eq!(c.replace_end, 2);
        assert!(!c.in_quoted_value);
    }

    #[test]
    fn cursor_mid_token_filters_correctly() {
        // Cursor after "da" in "daily" — should filter to daily only.
        let input = "daily";
        let c = reset_completions(input, 2); // cursor after "da"
        assert!(c.candidates.contains(&"daily".to_string()));
        assert_eq!(c.replace_start, 0);
        assert_eq!(c.replace_end, 2);
    }

    #[test]
    fn open_brace_offers_typed_skeletons() {
        let c = compl("{");
        // Primary candidate is `type` key; skeletons are secondary examples.
        assert!(c.candidates.contains(&"type".to_string()));
        assert!(c.examples.iter().any(|s| s.starts_with("{ type")));
    }

    #[test]
    fn in_type_value_offers_types() {
        let input = r#"{ type = "da" }"#;
        // Place cursor inside the quotes after "da"
        let cursor = input.find("da").unwrap() + 2;
        let c = reset_completions(input, cursor);
        assert!(c.candidates.contains(&"daily".to_string()));
        assert!(!c.candidates.contains(&"weekly".to_string()));
        assert!(c.in_quoted_value);
    }

    #[test]
    fn in_type_value_empty_offers_all_types() {
        let input = r#"{ type = "" }"#;
        let cursor = input.find("\"\"").unwrap() + 1; // inside empty quotes
        let c = reset_completions(input, cursor);
        assert_eq!(c.candidates, vec!["daily", "weekly", "interval", "schedule"]);
        assert!(c.in_quoted_value);
    }

    #[test]
    fn in_day_value_offers_weekdays() {
        let input = r#"{ type = "weekly", day = "mo" }"#;
        let cursor = input.find("\"mo\"").unwrap() + 3; // after "mo
        let c = reset_completions(input, cursor);
        assert!(c.candidates.contains(&"monday".to_string()));
        assert!(c.in_quoted_value);
    }

    #[test]
    fn key_name_completion_after_comma() {
        let input = r#"{ type = "interval", h }"#;
        let cursor = input.find(", h").unwrap() + 3; // after ", h"
        let c = reset_completions(input, cursor);
        assert!(c.candidates.contains(&"hours".to_string()));
        assert!(!c.in_quoted_value);
    }

    #[test]
    fn key_name_no_existing_key_repeated() {
        let input = r#"{ type = "interval", hours = 4, h }"#;
        let cursor = input.rfind(", h").unwrap() + 3;
        let c = reset_completions(input, cursor);
        // "h" prefix matches only "hours" — which is already present — so no candidates.
        assert!(!c.candidates.contains(&"hours".to_string()));
        // Try a prefix that matches a missing key.
        let input2 = r#"{ type = "interval", hours = 4, m }"#;
        let cursor2 = input2.rfind(", m").unwrap() + 3;
        let c2 = reset_completions(input2, cursor2);
        assert!(c2.candidates.contains(&"minutes".to_string()));
        assert!(!c2.candidates.contains(&"hours".to_string()));
    }

    #[test]
    fn after_comma_no_prefix_offers_missing_keys() {
        let input = r#"{ type = "daily", }"#;
        let cursor = input.find(", }").unwrap() + 2; // right after the comma+space
        let c = reset_completions(input, cursor);
        assert!(c.candidates.contains(&"time".to_string()));
        assert!(!c.candidates.contains(&"type".to_string())); // already present
    }

    #[test]
    fn extract_type_finds_type() {
        assert_eq!(
            extract_type(r#"{ type = "interval", hours = 4 }"#),
            Some("interval".to_string())
        );
    }

    #[test]
    fn extract_keys_finds_all() {
        let keys = extract_keys(r#"{ type = "interval", hours = 4 }"#);
        assert!(keys.contains(&"type".to_string()));
        assert!(keys.contains(&"hours".to_string()));
    }

    #[test]
    fn replace_range_is_correct_for_key_completion() {
        // "{ type = "interval", ho }" — cursor after "ho"
        let input = r#"{ type = "interval", ho }"#;
        let ho_pos = input.find("ho").unwrap();
        let cursor = ho_pos + 2;
        let c = reset_completions(input, cursor);
        assert_eq!(c.replace_start, ho_pos);
        assert_eq!(c.replace_end, cursor);
        assert!(c.candidates.contains(&"hours".to_string()));
    }

    #[test]
    fn after_equals_time_shows_hint() {
        // Cursor right after `time =` — no quote typed yet.
        // The hint is non-insertable, so it goes in examples, not candidates.
        let input = r#"{ type = "daily", time = }"#;
        let cursor = input.find("= }").unwrap() + 2; // after `= `
        let c = reset_completions(input, cursor);
        assert!(c.candidates.is_empty());
        assert_eq!(c.examples, vec![r#""<HH:MM>""#]);
        assert!(!c.in_quoted_value);
        assert_eq!(c.replace_start, cursor);
        assert_eq!(c.replace_end, cursor);
    }

    #[test]
    fn after_equals_type_shows_quoted_enum_values() {
        let input = r#"{ type = }"#;
        let cursor = input.find("= }").unwrap() + 2;
        let c = reset_completions(input, cursor);
        assert!(c.candidates.contains(&r#""daily""#.to_string()));
        assert!(c.candidates.contains(&r#""interval""#.to_string()));
        assert!(!c.in_quoted_value);
    }

    #[test]
    fn array_with_open_object_delegates_to_table_completion() {
        // Cursor inside `[{` — should offer table skeletons, not array templates.
        let input = r#"[{ type = "daily", t"#;
        let c = compl(input);
        // Should complete `t` as a key name — `time` is missing for daily.
        assert!(c.candidates.contains(&"time".to_string()));
        // Should NOT be an array template.
        assert!(!c.candidates.iter().any(|s| s.starts_with('[') && s.contains('}') ));
    }

    #[test]
    fn array_second_element_no_key_shows_type_not_first_element_keys() {
        // `[{ type = "interval", hours = 4 }, {|]` — cursor inside second object.
        // Should NOT bleed keys from the first element into the second.
        let input = r#"[{ type = "interval", hours = 4 }, {"#;
        let c = compl(input);
        // Second object has no type yet → `type` is the primary candidate.
        assert!(c.candidates.contains(&"type".to_string()));
        // Should NOT offer keys like `days`, `weeks` that belong to "interval"
        // from the first element.
        assert!(!c.candidates.contains(&"days".to_string()));
        assert!(!c.candidates.contains(&"weeks".to_string()));
        assert!(!c.candidates.contains(&"hours".to_string()));
    }

    #[test]
    fn array_second_element_after_comma_no_key_shows_type() {
        // `[{ type = "daily" }, { type = "interval", |]` — cursor after comma
        // in second element that already has `type`.
        let input = r#"[{ type = "daily" }, { type = "interval", "#;
        let c = compl(input);
        // Second object has type = "interval"; missing keys should be offered.
        // `hours`, `minutes`, `days`, `weeks` — NOT `type` (already present).
        assert!(c.candidates.contains(&"hours".to_string()));
        assert!(!c.candidates.contains(&"type".to_string()));
        // Should NOT see `time` (daily key) from the first element.
        assert!(!c.candidates.contains(&"time".to_string()));
    }

    #[test]
    fn array_without_object_still_offers_templates() {
        let c = compl("[");
        // Array templates are secondary examples; candidates is empty.
        assert!(c.candidates.is_empty());
        assert!(c.examples.iter().all(|s| s.starts_with('[')));
    }
}
