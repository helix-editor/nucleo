use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

use crate::Utf32String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaseMatching {
    Ignore,
    Smart,
    Respect,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[non_exhaustive]
pub enum PatternKind {
    Exact,
    Fuzzy,
    Substring,
    Prefix,
    Postfix,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct PatternAtom {
    kind: PatternKind,
    needle: Utf32String,
    invert: bool,
    ignore_case: bool,
}
impl PatternAtom {
    fn literal(
        needle: &str,
        normalize: bool,
        case: CaseMatching,
        kind: PatternKind,
        escape_whitespace: bool,
    ) -> PatternAtom {
        let mut ignore_case = case == CaseMatching::Ignore;
        let needle = if needle.is_ascii() {
            let mut needle = if escape_whitespace {
                if let Some((start, rem)) = needle.split_once("\\ ") {
                    let mut needle = start.to_owned();
                    for rem in rem.split("\\ ") {
                        needle.push(' ');
                        needle.push_str(rem);
                    }
                    needle
                } else {
                    needle.to_owned()
                }
            } else {
                needle.to_owned()
            };

            match case {
                CaseMatching::Ignore => needle.make_ascii_lowercase(),
                CaseMatching::Smart => ignore_case = needle.bytes().any(|b| b.is_ascii_uppercase()),
                CaseMatching::Respect => (),
            }

            Utf32String::Ascii(needle.into_boxed_str())
        } else {
            let mut needle_ = Vec::with_capacity(needle.len());
            if escape_whitespace {
                let mut saw_backslash = false;
                for mut c in needle.chars() {
                    if saw_backslash {
                        if c == ' ' {
                            needle_.push(' ');
                            saw_backslash = false;
                            continue;
                        } else {
                            needle_.push('\\');
                        }
                    }
                    saw_backslash = c == '\\';
                    if normalize {
                        c = chars::normalize(c);
                    }
                    match case {
                        CaseMatching::Ignore => c = chars::to_lower_case(c),
                        CaseMatching::Smart => {
                            ignore_case = ignore_case || c.is_uppercase();
                        }
                        CaseMatching::Respect => (),
                    }
                    needle_.push(c);
                }
            };
            Utf32String::Unicode(needle_.into_boxed_slice())
        };
        PatternAtom {
            kind,
            needle,
            invert: false,
            ignore_case,
        }
    }

    fn parse(raw: &str, normalize: bool, case: CaseMatching) -> PatternAtom {
        let mut atom = raw;
        let inverse = atom.starts_with('!');
        if inverse {
            atom = &atom[1..];
        }

        let mut kind = match atom.as_bytes() {
            [b'^', ..] => {
                atom = &atom[1..];
                PatternKind::Prefix
            }
            [b'\'', ..] => {
                atom = &atom[1..];
                PatternKind::Substring
            }
            [b'\\', b'^' | b'\'', ..] => {
                atom = &atom[1..];
                PatternKind::Fuzzy
            }
            _ => PatternKind::Fuzzy,
        };

        match atom.as_bytes() {
            [.., b'\\', b'$'] => (),
            [.., b'$'] => {
                kind = if kind == PatternKind::Fuzzy {
                    PatternKind::Postfix
                } else {
                    PatternKind::Exact
                };
                atom = &atom[..atom.len() - 1]
            }
            _ => (),
        }

        if inverse && kind == PatternKind::Fuzzy {
            kind = PatternKind::Substring
        }

        PatternAtom::literal(atom, normalize, case, kind, true)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub enum Status {
    Unchanged,
    Update,
    Rescore,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub cols: Vec<Pattern>,
}

impl Query {
    pub fn new(matcher_config: &MatcherConfig, case_matching: CaseMatching, cols: usize) -> Query {
        Query {
            cols: vec![
                Pattern {
                    terms: Vec::new(),
                    case_matching,
                    normalize: matcher_config.normalize,
                    status: Status::Unchanged,
                };
                cols
            ],
        }
    }

    pub(crate) fn status(&self) -> Status {
        self.cols
            .iter()
            .map(|col| col.status)
            .max()
            .unwrap_or(Status::Unchanged)
    }

    pub(crate) fn reset_status(&mut self) {
        for col in &mut self.cols {
            col.status = Status::Unchanged
        }
    }

    pub fn score(&self, haystack: &[Utf32String], matcher: &mut Matcher) -> Option<u32> {
        // TODO: wheight columns?
        let mut score = 0;
        for (pattern, haystack) in self.cols.iter().zip(haystack) {
            score += pattern.score(haystack.slice(..), matcher)?
        }
        Some(score)
    }
}

#[derive(Clone, Debug)]
pub struct Pattern {
    terms: Vec<PatternAtom>,
    case_matching: CaseMatching,
    normalize: bool,
    status: Status,
}

impl Pattern {
    pub(crate) fn score(&self, haystack: Utf32Str<'_>, matcher: &mut Matcher) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(0);
        }
        let mut score = 0;
        for pattern in &self.terms {
            matcher.config.ignore_case = pattern.ignore_case;
            let pattern_score = match pattern.kind {
                PatternKind::Exact => matcher.exact_match(haystack, pattern.needle.slice(..)),
                PatternKind::Fuzzy => matcher.fuzzy_match(haystack, pattern.needle.slice(..)),
                PatternKind::Substring => {
                    matcher.substring_match(haystack, pattern.needle.slice(..))
                }
                PatternKind::Prefix => matcher.prefix_match(haystack, pattern.needle.slice(..)),
                PatternKind::Postfix => matcher.prefix_match(haystack, pattern.needle.slice(..)),
            };
            if pattern.invert {
                if pattern_score.is_some() {
                    return None;
                }
            } else {
                score += pattern_score? as u32
            }
        }
        Some(score)
    }

    pub fn indices(
        &self,
        haystack: Utf32Str<'_>,
        matcher: &mut Matcher,
        indices: &mut Vec<u32>,
    ) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(0);
        }
        let mut score = 0;
        for pattern in &self.terms {
            matcher.config.ignore_case = pattern.ignore_case;
            if pattern.invert {
                let pattern_score = match pattern.kind {
                    PatternKind::Exact => matcher.exact_match(haystack, pattern.needle.slice(..)),
                    PatternKind::Fuzzy => matcher.fuzzy_match(haystack, pattern.needle.slice(..)),
                    PatternKind::Substring => {
                        matcher.substring_match(haystack, pattern.needle.slice(..))
                    }
                    PatternKind::Prefix => matcher.prefix_match(haystack, pattern.needle.slice(..)),
                    PatternKind::Postfix => {
                        matcher.prefix_match(haystack, pattern.needle.slice(..))
                    }
                };
                if pattern_score.is_some() {
                    return None;
                }
                continue;
            }
            let pattern_score = match pattern.kind {
                PatternKind::Exact => {
                    matcher.exact_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Fuzzy => {
                    matcher.exact_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Substring => {
                    matcher.exact_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Prefix => {
                    matcher.exact_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Postfix => {
                    matcher.exact_indices(haystack, pattern.needle.slice(..), indices)
                }
            };
            score += pattern_score? as u32
        }
        Some(score)
    }

    pub fn parse_from(&mut self, pattern: &str, append: bool) {
        self.terms.clear();
        let invert = self.terms.last().map_or(false, |pat| pat.invert);
        for atom in pattern_atoms(pattern) {
            self.terms
                .push(PatternAtom::parse(atom, self.normalize, self.case_matching));
        }
        self.status = if append && !invert && self.status != Status::Rescore {
            Status::Update
        } else {
            Status::Rescore
        };
    }

    pub fn set_literal(&mut self, pattern: &str, kind: PatternKind, append: bool) {
        self.terms.clear();
        let pattern =
            PatternAtom::literal(pattern, self.normalize, self.case_matching, kind, false);
        self.terms.push(pattern);
        self.status = if append && self.status != Status::Rescore {
            Status::Update
        } else {
            Status::Rescore
        };
    }
}

fn pattern_atoms(pattern: &str) -> impl Iterator<Item = &str> + '_ {
    let mut saw_backslash = false;
    pattern.split(move |c| {
        saw_backslash = match c {
            ' ' if !saw_backslash => return true,
            '\\' => true,
            _ => false,
        };
        false
    })
}
