use nucleo_matcher::{chars, Matcher, MatcherConfig, Utf32Str};

#[cfg(test)]
mod tests;

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
        let mut ignore_case;
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
                CaseMatching::Ignore => {
                    ignore_case = true;
                    needle.make_ascii_lowercase()
                }
                CaseMatching::Smart => {
                    ignore_case = !needle.bytes().any(|b| b.is_ascii_uppercase())
                }
                CaseMatching::Respect => ignore_case = false,
            }

            Utf32String::Ascii(needle.into_boxed_str())
        } else {
            let mut needle_ = Vec::with_capacity(needle.len());
            ignore_case = matches!(case, CaseMatching::Ignore | CaseMatching::Smart);
            if escape_whitespace {
                let mut saw_backslash = false;
                for mut c in chars::graphemes(needle) {
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
                            ignore_case = ignore_case && !chars::is_upper_case(c)
                        }
                        CaseMatching::Respect => (),
                    }
                    needle_.push(c);
                }
            } else {
                let chars = chars::graphemes(needle).map(|mut c| {
                    if normalize {
                        c = chars::normalize(c);
                    }
                    match case {
                        CaseMatching::Ignore => c = chars::to_lower_case(c),
                        CaseMatching::Smart => {
                            ignore_case = ignore_case && !chars::is_upper_case(c);
                        }
                        CaseMatching::Respect => (),
                    }
                    c
                });
                needle_.extend(chars);
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
        let invert = match atom.as_bytes() {
            [b'!', ..] => {
                atom = &atom[1..];
                true
            }
            [b'\\', b'!', ..] => {
                atom = &atom[1..];
                false
            }
            _ => false,
        };

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

        let mut append_dollar = false;
        match atom.as_bytes() {
            [.., b'\\', b'$'] => {
                append_dollar = true;
                atom = &atom[..atom.len() - 2]
            }
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

        if invert && kind == PatternKind::Fuzzy {
            kind = PatternKind::Substring
        }

        let mut pattern = PatternAtom::literal(atom, normalize, case, kind, true);
        pattern.invert = invert;
        if append_dollar {
            pattern.needle.push('$');
        }
        pattern
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub enum Status {
    Unchanged,
    Update,
    Rescore,
}

#[derive(Debug)]
pub struct MultiPattern {
    pub cols: Vec<Pattern>,
}

impl Clone for MultiPattern {
    fn clone(&self) -> Self {
        Self {
            cols: self.cols.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.cols.clone_from(&source.cols)
    }
}

impl MultiPattern {
    pub fn new(
        matcher_config: &MatcherConfig,
        case_matching: CaseMatching,
        columns: usize,
    ) -> MultiPattern {
        MultiPattern {
            cols: vec![Pattern::new(matcher_config, case_matching); columns],
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

#[derive(Debug)]
pub struct Pattern {
    atoms: Vec<PatternAtom>,
    case_matching: CaseMatching,
    normalize: bool,
    status: Status,
}

impl Pattern {
    pub fn new(matcher_config: &MatcherConfig, case_matching: CaseMatching) -> Pattern {
        Pattern {
            atoms: Vec::new(),
            case_matching,
            normalize: matcher_config.normalize,
            status: Status::Unchanged,
        }
    }
    pub fn new_fuzzy_literal(
        matcher_config: &MatcherConfig,
        case_matching: CaseMatching,
        pattern: &str,
    ) -> Pattern {
        let mut res = Pattern {
            atoms: Vec::new(),
            case_matching,
            normalize: matcher_config.normalize,
            status: Status::Unchanged,
        };
        res.set_literal(pattern, PatternKind::Fuzzy, false);
        res
    }

    pub fn score(&self, haystack: Utf32Str<'_>, matcher: &mut Matcher) -> Option<u32> {
        if self.atoms.is_empty() {
            return Some(0);
        }
        let mut score = 0;
        for pattern in &self.atoms {
            matcher.config.ignore_case = pattern.ignore_case;
            let pattern_score = match pattern.kind {
                PatternKind::Exact => matcher.exact_match(haystack, pattern.needle.slice(..)),
                PatternKind::Fuzzy => matcher.fuzzy_match(haystack, pattern.needle.slice(..)),
                PatternKind::Substring => {
                    matcher.substring_match(haystack, pattern.needle.slice(..))
                }
                PatternKind::Prefix => matcher.prefix_match(haystack, pattern.needle.slice(..)),
                PatternKind::Postfix => matcher.postfix_match(haystack, pattern.needle.slice(..)),
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
        if self.atoms.is_empty() {
            return Some(0);
        }
        let mut score = 0;
        for pattern in &self.atoms {
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
                        matcher.postfix_match(haystack, pattern.needle.slice(..))
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
                    matcher.fuzzy_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Substring => {
                    matcher.substring_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Prefix => {
                    matcher.prefix_indices(haystack, pattern.needle.slice(..), indices)
                }
                PatternKind::Postfix => {
                    matcher.postfix_indices(haystack, pattern.needle.slice(..), indices)
                }
            };
            score += pattern_score? as u32
        }
        Some(score)
    }

    pub fn parse_from(&mut self, pattern: &str, append: bool) {
        let invert = self.atoms.last().map_or(false, |pat| pat.invert);
        self.atoms.clear();
        let atoms = pattern_atoms(pattern).filter_map(|atom| {
            let atom = PatternAtom::parse(atom, self.normalize, self.case_matching);
            if atom.needle.is_empty() {
                return None;
            }
            Some(atom)
        });
        self.atoms.extend(atoms);

        self.status = if append && !invert && self.status != Status::Rescore {
            Status::Update
        } else {
            Status::Rescore
        };
    }

    pub fn set_literal(&mut self, pattern: &str, kind: PatternKind, append: bool) {
        self.atoms.clear();
        let pattern =
            PatternAtom::literal(pattern, self.normalize, self.case_matching, kind, false);
        if !pattern.needle.is_empty() {
            self.atoms.push(pattern);
        }
        self.status = if append && self.status != Status::Rescore {
            Status::Update
        } else {
            Status::Rescore
        };
    }

    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }
}

impl Clone for Pattern {
    fn clone(&self) -> Self {
        Self {
            atoms: self.atoms.clone(),
            case_matching: self.case_matching,
            normalize: self.normalize,
            status: self.status,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.atoms.clone_from(&source.atoms);
        self.case_matching = source.case_matching;
        self.normalize = source.normalize;
        self.status = source.status;
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
