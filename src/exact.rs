use memchr::{Memchr, Memchr2};

use crate::chars::{AsciiChar, Char};
use crate::score::{BONUS_FIRST_CHAR_MULTIPLIER, SCORE_MATCH};
use crate::Matcher;

impl Matcher {
    pub(crate) fn substring_match_1_ascii<const INDICES: bool>(
        &mut self,
        haystack: &[u8],
        c: u8,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        let mut max_score = 0;
        let mut max_pos = 0;
        if self.config.ignore_case && c >= b'a' && c <= b'z' {
            for i in Memchr2::new(c, c - 32, haystack) {
                let prev_char_class = i
                    .checked_sub(1)
                    .map(|i| AsciiChar(haystack[i]).char_class(&self.config))
                    .unwrap_or(self.config.initial_char_class);
                let char_class = AsciiChar(haystack[i]).char_class(&self.config);
                let bonus = self.config.bonus_for(prev_char_class, char_class);
                let score = bonus * BONUS_FIRST_CHAR_MULTIPLIER + SCORE_MATCH;
                if score > max_score {
                    max_pos = i as u32;
                    max_score = score;
                    // can't get better than this
                    if score >= self.config.bonus_boundary_white
                        && score >= self.config.bonus_boundary_delimiter
                    {
                        break;
                    }
                }
            }
        } else {
            let char_class = AsciiChar(c).char_class(&self.config);
            for i in Memchr::new(c, haystack) {
                let prev_char_class = i
                    .checked_sub(1)
                    .map(|i| AsciiChar(haystack[i]).char_class(&self.config))
                    .unwrap_or(self.config.initial_char_class);
                let bonus = self.config.bonus_for(prev_char_class, char_class);
                let score = bonus * BONUS_FIRST_CHAR_MULTIPLIER + SCORE_MATCH;
                if score > max_score {
                    max_pos = i as u32;
                    max_score = score;
                    // can't get better than this
                    if score >= self.config.bonus_boundary_white
                        && score >= self.config.bonus_boundary_delimiter
                    {
                        break;
                    }
                }
            }
        }
        if max_score == 0 {
            return None;
        }

        if INDICES {
            indices.clear();
            indices.push(max_pos);
        }
        Some(max_score)
    }

    pub(crate) fn substring_match_1_non_ascii<const INDICES: bool>(
        &mut self,
        haystack: &[char],
        needle: char,
        start: usize,
        indices: &mut Vec<u32>,
    ) -> u16 {
        let mut max_score = 0;
        let mut max_pos = 0;
        let mut prev_class = start
            .checked_sub(1)
            .map(|i| haystack[i].char_class(&self.config))
            .unwrap_or(self.config.initial_char_class);
        for (i, &c) in haystack.iter().enumerate() {
            let (c, char_class) = c.char_class_and_normalize(&self.config);
            if c != needle {
                continue;
            }
            let bonus = self.config.bonus_for(prev_class, char_class);
            prev_class = char_class;
            let score = bonus * BONUS_FIRST_CHAR_MULTIPLIER + SCORE_MATCH;
            if score > max_score {
                max_pos = i as u32;
                max_score = score;
                // can't get better than this
                if score >= self.config.bonus_boundary_white
                    && score >= self.config.bonus_boundary_delimiter
                {
                    break;
                }
            }
        }

        if INDICES {
            indices.clear();
            indices.push(max_pos);
        }
        max_score
    }
}
