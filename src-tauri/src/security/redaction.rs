//! Redaction for the shareable end-user summary report. Replaces account
//! names, the computer name, profile paths, UNC hosts and e-mail addresses
//! with stable placeholders. Detailed technician reports are never redacted
//! and stay local by default.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    /// lower-case term -> placeholder; applied longest-first.
    terms: BTreeMap<String, String>,
    users: usize,
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn computer_name(mut self, name: &str) -> Self {
        self.add(name, "[computer]");
        self
    }

    /// Register a user; profile paths and account names map to `[user N]`.
    pub fn user(mut self, account_name: &str, profile_path: &str) -> Self {
        self.users += 1;
        let tag = format!("[user {}]", self.users);
        self.add(profile_path, &format!("[profile of {tag}]"));
        // Account names may be DOMAIN\name; redact both forms.
        self.add(account_name, &tag);
        if let Some((_, short)) = account_name.rsplit_once('\\') {
            self.add(short, &tag);
        }
        self
    }

    pub fn term(mut self, term: &str, placeholder: &str) -> Self {
        self.add(term, placeholder);
        self
    }

    fn add(&mut self, term: &str, placeholder: &str) {
        let t = term.trim();
        // Very short terms would redact ordinary words; skip them.
        if t.chars().count() >= 3 {
            self.terms.entry(t.to_lowercase()).or_insert_with(|| placeholder.to_string());
        }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut out = redact_unc_hosts(&redact_emails(input));
        let mut terms: Vec<(&String, &String)> = self.terms.iter().collect();
        terms.sort_by_key(|(t, _)| std::cmp::Reverse(t.len()));
        for (term, placeholder) in terms {
            out = replace_case_insensitive(&out, term, placeholder);
        }
        out
    }
}

fn replace_case_insensitive(haystack: &str, needle_lower: &str, replacement: &str) -> String {
    if needle_lower.is_empty() {
        return haystack.to_string();
    }
    let lower = haystack.to_lowercase();
    // Lower-casing can change byte lengths for some scripts; fall back to an
    // exact match in that case rather than risk slicing mid-character.
    if lower.len() != haystack.len() {
        return haystack.replace(needle_lower, replacement);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while let Some(pos) = lower[i..].find(needle_lower) {
        let start = i + pos;
        let end = start + needle_lower.len();
        let before_ok = start == 0 || !is_word_char(lower[..start].chars().next_back().unwrap());
        let after_ok = end == lower.len() || !is_word_char(lower[end..].chars().next().unwrap());
        out.push_str(&haystack[i..start]);
        if before_ok && after_ok {
            out.push_str(replacement);
        } else {
            out.push_str(&haystack[start..end]);
        }
        i = end;
    }
    out.push_str(&haystack[i..]);
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn redact_emails(input: &str) -> String {
    input
        .split_inclusive(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"' || c == '\'')
        .map(|tok| {
            let core = tok.trim_end_matches(|c: char| c.is_whitespace() || "<>\"'".contains(c));
            let tail = &tok[core.len()..];
            match core.split_once('@') {
                Some((a, b)) if !a.is_empty() && b.contains('.') && !core.contains('\\') && !core.contains('/') => format!("[email]{tail}"),
                _ => tok.to_string(),
            }
        })
        .collect()
}

/// `\\server\share\path` -> `\\[server]\share\path`
fn redact_unc_hosts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find(r"\\") {
        out.push_str(&rest[..pos + 2]);
        let after = &rest[pos + 2..];
        let end = after.find(|c: char| c == '\\' || c.is_whitespace() || c == '<' || c == '"').unwrap_or(after.len());
        if end > 0 && !after.starts_with('?') && !after.starts_with('.') {
            out.push_str("[server]");
        } else {
            out.push_str(&after[..end]);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_users_paths_computer_and_emails() {
        let r = Redactor::new().computer_name("ACCT-PC-07").user("CONTOSO\\jdoe", "C:\\Users\\jdoe");
        let s = r.redact("Captured C:\\Users\\jdoe\\Documents for CONTOSO\\jdoe on ACCT-PC-07; contact jdoe@contoso.com");
        assert!(!s.to_lowercase().contains("jdoe"), "{s}");
        assert!(!s.contains("ACCT-PC-07"));
        assert!(s.contains("[profile of [user 1]]\\Documents"), "{s}");
        assert!(s.contains("[computer]"));
        assert!(s.contains("[email]"));
    }

    #[test]
    fn does_not_redact_inside_words() {
        let r = Redactor::new().user("ann", "C:\\Users\\ann");
        assert_eq!(r.redact("Annual planning by ann"), "Annual planning by [user 1]");
    }

    #[test]
    fn redacts_unc_servers() {
        let r = Redactor::new();
        assert_eq!(r.redact(r"Mapped Z: to \\fileserver01\finance"), r"Mapped Z: to \\[server]\finance");
    }

    #[test]
    fn multiple_users_get_distinct_tags() {
        let r = Redactor::new().user("alice", "C:\\Users\\alice").user("bob", "C:\\Users\\bob");
        let s = r.redact("alice and bob");
        assert_eq!(s, "[user 1] and [user 2]");
    }
}
