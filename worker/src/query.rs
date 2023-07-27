use nucleo_matcher::{Matcher, Utf32Str};

use crate::Utf32String;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PatternKind {
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
    pub(crate) fn status(&self) -> Status {
        self.cols
            .iter()
            .map(|col| col.status)
            .max()
            .unwrap_or(Status::Unchanged)
    }

    pub(crate) fn score(&self, haystack: &[Utf32String], matcher: &mut Matcher) -> Option<u32> {
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
    status: Status,
}

impl Pattern {
    pub(crate) fn score(&self, haystack: Utf32Str<'_>, matcher: &mut Matcher) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(0);
        }
        let mut score = 0;
        for pattern in &self.terms {
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
}
