use std::cmp::max;

use crate::chars::{Char, CharClass};
use crate::{Matcher, MatcherConfig};

pub(crate) const SCORE_MATCH: u16 = 16;
pub(crate) const PENALTY_GAP_START: u16 = 3;
pub(crate) const PENALTY_GAP_EXTENSION: u16 = 1;

// We prefer matches at the beginning of a word, but the bonus should not be
// too great to prevent the longer acronym matches from always winning over
// shorter fuzzy matches. The bonus point here was specifically chosen that
// the bonus is cancelled when the gap between the acronyms grows over
// 8 characters, which is approximately the average length of the words found
// in web2 dictionary and my file system.
pub(crate) const BONUS_BOUNDARY: u16 = SCORE_MATCH / 2;

// Although bonus point for non-word characters is non-contextual, we need it
// for computing bonus points for consecutive chunks starting with a non-word
// character.
pub(crate) const BONUS_NON_WORD: u16 = SCORE_MATCH / 2;

// Edge-triggered bonus for matches in camelCase words.
// Compared to word-boundary case, they don't accompany single-character gaps
// (e.g. FooBar vs. foo-bar), so we deduct bonus point accordingly.
pub(crate) const BONUS_CAMEL123: u16 = BONUS_BOUNDARY - PENALTY_GAP_EXTENSION;

// Minimum bonus point given to characters in consecutive chunks.
// Note that bonus points for consecutive matches shouldn't have needed if we
// used fixed match score as in the original algorithm.
pub(crate) const BONUS_CONSECUTIVE: u16 = PENALTY_GAP_START + PENALTY_GAP_EXTENSION;

// The first character in the typed pattern usually has more significance
// than the rest so it's important that it appears at special positions where
// bonus points are given, e.g. "to-go" vs. "ongoing" on "og" or on "ogo".
// The amount of the extra bonus should be limited so that the gap penalty is
// still respected.
pub(crate) const BONUS_FIRST_CHAR_MULTIPLIER: u16 = 2;

impl MatcherConfig {
    #[inline]
    pub(crate) fn bonus_for(&self, prev_class: CharClass, class: CharClass) -> u16 {
        if class > CharClass::NonWord {
            // transition from non word to word
            match prev_class {
                CharClass::Whitespace => return self.bonus_boundary_white,
                CharClass::Delimiter => return self.bonus_boundary_delimiter,
                CharClass::NonWord => return BONUS_BOUNDARY,
                _ => (),
            }
        }
        if prev_class == CharClass::Lower && class == CharClass::Upper
            || prev_class != CharClass::Number && class == CharClass::Number
        {
            // camelCase letter123
            BONUS_CAMEL123
        } else if class == CharClass::NonWord {
            BONUS_NON_WORD
        } else if class == CharClass::Whitespace {
            self.bonus_boundary_white
        } else {
            0
        }
    }
}
impl Matcher {
    #[inline(always)]
    pub(crate) fn bonus_for(&self, prev_class: CharClass, class: CharClass) -> u16 {
        self.config.bonus_for(prev_class, class)
    }

    pub(crate) fn calculate_score<const INDICES: bool, H: Char + PartialEq<N>, N: Char>(
        &mut self,
        haystack: &[H],
        needle: &[N],
        start: usize,
        end: usize,
        indices: &mut Vec<u32>,
    ) -> u16 {
        if INDICES {
            indices.reserve(needle.len());
        }

        let mut prev_class = start
            .checked_sub(1)
            .map(|i| haystack[i].char_class(&self.config))
            .unwrap_or(self.config.initial_char_class);
        let mut needle_iter = needle.iter();
        let mut needle_char = *needle_iter.next().unwrap();

        let mut in_gap = false;
        let mut consecutive = 1;

        // unrolled the firs iteration to make applying the first char multiplier less akward
        if INDICES {
            indices.push(start as u32)
        }
        let mut first_bonus = self.bonus_for(prev_class, haystack[0].char_class(&self.config));
        let mut score = SCORE_MATCH + first_bonus * BONUS_FIRST_CHAR_MULTIPLIER;

        for (i, c) in haystack[start + 1..end].iter().enumerate() {
            let class = c.char_class(&self.config);
            let c = c.normalize(&self.config);
            if c == needle_char {
                if INDICES {
                    indices.push(i as u32 + start as u32)
                }
                let mut bonus = self.bonus_for(prev_class, class);
                if consecutive == 0 {
                    first_bonus = bonus
                } else {
                    // Break consecutive chunk
                    if bonus > first_bonus {
                        if bonus >= BONUS_BOUNDARY {
                            first_bonus = bonus;
                        } else {
                            bonus = max(bonus, BONUS_CONSECUTIVE);
                        }
                    } else {
                        bonus = max(first_bonus, BONUS_CONSECUTIVE);
                    }
                }
                score += SCORE_MATCH + bonus;
                in_gap = false;
                consecutive += 1;
                if let Some(&next) = needle_iter.next() {
                    needle_char = next;
                }
            } else {
                let penalty = if in_gap {
                    PENALTY_GAP_EXTENSION
                } else {
                    PENALTY_GAP_START
                };
                score = score.saturating_sub(penalty);
                in_gap = true;
                consecutive = 0;
                first_bonus = 0;
            }
            prev_class = class;
        }

        score
    }
}
