use crate::chars::Char;
use crate::Matcher;

impl Matcher {
    /// greedy fallback algoritm, much faster (linear time) but reported scores/indicies
    /// might not be the best match
    pub(crate) fn fuzzy_match_greedy<const INDICIES: bool, H: Char + PartialEq<N>, N: Char>(
        &mut self,
        haystack: &[H],
        needle: &[N],
        mut start: usize,
        mut end: usize,
        indicies: &mut Vec<u32>,
    ) -> Option<u16> {
        let first_char_end = if H::ASCII { start + 1 } else { end };
        if !H::ASCII && needle.len() != 1 {
            let mut needle_iter = needle[1..].iter().copied();
            if let Some(mut needle_char) = needle_iter.next() {
                for (i, &c) in haystack[first_char_end..].iter().enumerate() {
                    if c.normalize(&self.config) == needle_char {
                        let Some(next_needle_char) = needle_iter.next() else {
                            end = i + 1;
                            break;
                        };
                        needle_char = next_needle_char;
                    }
                }
            }
        }
        // mimimize the greedly match by greedy matching in reverse

        let mut needle_iter = needle.iter().rev().copied();
        let mut needle_char = needle_iter.next().unwrap();
        for (i, &c) in haystack[start..end].iter().enumerate().rev() {
            println!("{c:?} {i} {needle_char:?}");
            if c == needle_char {
                let Some(next_needle_char) = needle_iter.next() else {
                    start += i;
                    break;
                };
                needle_char = next_needle_char;
            }
        }
        Some(self.calculate_score::<INDICIES, H, N>(haystack, needle, start, end, indicies))
    }
}
