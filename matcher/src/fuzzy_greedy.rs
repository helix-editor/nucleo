use crate::chars::Char;
use crate::Matcher;

impl Matcher {
    /// greedy fallback algorithm, much faster (linear time) but reported scores/indicies
    /// might not be the best match
    pub(crate) fn fuzzy_match_greedy_<const INDICES: bool, H: Char + PartialEq<N>, N: Char>(
        &mut self,
        haystack: &[H],
        needle: &[N],
        mut start: usize,
        mut end: usize,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        // Normalize each needle char at the consumption point so
        // mixed-case needles match against a normalized haystack under
        // `ignore_case` without panicking downstream — mirrors the seam
        // fix in `fuzzy_optimal::setup`.
        let first_char_end = if H::ASCII && N::ASCII { start + 1 } else { end };
        'nonascii: {
            if !H::ASCII || !N::ASCII {
                let mut needle_iter = needle[1..].iter().copied();
                if let Some(raw) = needle_iter.next() {
                    let mut needle_char = raw.normalize(&self.config);
                    for (i, &c) in haystack[first_char_end..].iter().enumerate() {
                        if c.normalize(&self.config) == needle_char {
                            let Some(raw_next) = needle_iter.next() else {
                                // we found a match so we are now in the same state
                                // as the prefilter would produce
                                end = first_char_end + i + 1;
                                break 'nonascii;
                            };
                            needle_char = raw_next.normalize(&self.config);
                        }
                    }
                    // some needle chars were not matched bail out
                    return None;
                }
            }
        } // minimize the greedly match by greedy matching in reverse

        let mut needle_iter = needle.iter().rev().copied();
        let mut needle_char = needle_iter.next().unwrap().normalize(&self.config);
        for (i, &c) in haystack[start..end].iter().enumerate().rev() {
            let c = c.normalize(&self.config);
            if c == needle_char {
                let Some(raw_next_needle_char) = needle_iter.next() else {
                    start += i;
                    break;
                };
                needle_char = raw_next_needle_char.normalize(&self.config);
            }
        }
        Some(self.calculate_score::<INDICES, H, N>(haystack, needle, start, end, indices))
    }
}
