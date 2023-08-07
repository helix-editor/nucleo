use crate::pattern::{PatternAtom, Status};
use crate::{CaseMatching, Pattern, PatternKind};

fn parse_atom(pat: &str) -> PatternAtom {
    parse_atom_with(pat, CaseMatching::Smart)
}

fn parse_atom_with(pat: &str, case_matching: CaseMatching) -> PatternAtom {
    let mut pat = parse_with(pat, case_matching, false);
    assert_eq!(pat.atoms.len(), 1);
    pat.atoms.remove(0)
}

fn parse_with(pat: &str, case_matching: CaseMatching, append: bool) -> Pattern {
    let mut res = Pattern::new(&nucleo_matcher::MatcherConfig::DEFAULT, case_matching);
    res.parse_from(pat, append);
    res
}

#[test]
fn negative() {
    let pat = parse_atom("!foo");
    assert!(pat.invert);
    assert_eq!(pat.kind, PatternKind::Substring);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("!^foo");
    assert!(pat.invert);
    assert_eq!(pat.kind, PatternKind::Prefix);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("!foo$");
    assert!(pat.invert);
    assert_eq!(pat.kind, PatternKind::Postfix);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("!^foo$");
    assert!(pat.invert);
    assert_eq!(pat.kind, PatternKind::Exact);
    assert_eq!(pat.needle.to_string(), "foo");
}

#[test]
fn pattern_kinds() {
    let pat = parse_atom("foo");
    assert!(!pat.invert);
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("'foo");
    assert!(!pat.invert);
    assert_eq!(pat.kind, PatternKind::Substring);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("^foo");
    assert!(!pat.invert);
    assert_eq!(pat.kind, PatternKind::Prefix);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("foo$");
    assert!(!pat.invert);
    assert_eq!(pat.kind, PatternKind::Postfix);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom("^foo$");
    assert!(!pat.invert);
    assert_eq!(pat.kind, PatternKind::Exact);
    assert_eq!(pat.needle.to_string(), "foo");
}

#[test]
fn case_matching() {
    let pat = parse_atom_with("foo", CaseMatching::Smart);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom_with("Foo", CaseMatching::Smart);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Foo");
    let pat = parse_atom_with("Foo", CaseMatching::Ignore);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "foo");
    let pat = parse_atom_with("Foo", CaseMatching::Respect);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Foo");
    let pat = parse_atom_with("Foo", CaseMatching::Respect);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Foo");
    let pat = parse_atom_with("Äxx", CaseMatching::Ignore);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "axx");
    let pat = parse_atom_with("Äxx", CaseMatching::Respect);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Axx");
    let pat = parse_atom_with("Äxx", CaseMatching::Smart);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Axx");
    let pat = parse_atom_with("Äxx", CaseMatching::Smart);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Axx");
    let pat = parse_atom_with("你xx", CaseMatching::Smart);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "你xx");
    let pat = parse_atom_with("你xx", CaseMatching::Ignore);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "你xx");
    let pat = parse_atom_with("Ⲽxx", CaseMatching::Smart);
    assert!(!pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "Ⲽxx");
    let pat = parse_atom_with("Ⲽxx", CaseMatching::Ignore);
    assert!(pat.ignore_case);
    assert_eq!(pat.needle.to_string(), "ⲽxx");
}

#[test]
fn escape() {
    let pat = parse_atom("foo\\ bar");
    assert_eq!(pat.needle.to_string(), "foo bar");
    let pat = parse_atom("\\!foo");
    assert_eq!(pat.needle.to_string(), "!foo");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("\\'foo");
    assert_eq!(pat.needle.to_string(), "'foo");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("\\^foo");
    assert_eq!(pat.needle.to_string(), "^foo");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("foo\\$");
    assert_eq!(pat.needle.to_string(), "foo$");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("^foo\\$");
    assert_eq!(pat.needle.to_string(), "foo$");
    assert_eq!(pat.kind, PatternKind::Prefix);
    let pat = parse_atom("\\^foo\\$");
    assert_eq!(pat.needle.to_string(), "^foo$");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("\\!^foo\\$");
    assert_eq!(pat.needle.to_string(), "!^foo$");
    assert_eq!(pat.kind, PatternKind::Fuzzy);
    let pat = parse_atom("!\\^foo\\$");
    assert_eq!(pat.needle.to_string(), "^foo$");
    assert_eq!(pat.kind, PatternKind::Substring);
}

#[test]
fn append() {
    let mut pat = parse_with("!", CaseMatching::Smart, true);
    assert_eq!(pat.status, Status::Update);
    pat.parse_from("!f", true);
    assert_eq!(pat.status, Status::Update);
    pat.parse_from("!fo", true);
    assert_eq!(pat.status, Status::Rescore);
}
