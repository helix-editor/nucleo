/*!
`nucleo_matcher` is a low level crate that contains the matcher implementation
used by the other nucleo crates.

The matcher is hightly optimized and can significantly outperform `fzf` and
`skim` (the `fuzzy-matcher` crate). However some of these optimizations require
a slightly less convenient API. Particularly, `nucleo_matcher` requires that
needles and haystacks are provided as [UTF32 strings](crate::Utf32Str) instead
of rusts normal utf32 strings.
*/

// sadly ranges don't optmimzie well
#![allow(clippy::manual_range_contains)]

pub mod chars;
mod config;
#[cfg(test)]
mod debug;
mod exact;
mod fuzzy_greedy;
mod fuzzy_optimal;
mod matrix;
mod prefilter;
mod score;
mod utf32_str;

#[cfg(test)]
mod tests;

pub use crate::config::MatcherConfig;
pub use crate::utf32_str::Utf32Str;

use crate::chars::{AsciiChar, Char};
use crate::matrix::MatrixSlab;

/// A matcher engine that can execute (fuzzy) matches.
///
/// A matches contains **heap allocated** scratch memory that is reused during
/// matching. This scratch memory allows the matcher to garunte that it will
/// **never allocate** during matching (with the exception of pushing to the
/// `indices` vector if there isn't enough capacity). However this scratch
/// memory is fairly large (around 135KB) so creating a matcher is expensive and
/// should be reused.
///
/// All `.._match` functions will not compute the indices of the matched chars
/// and are therefore significantly faster. These should be used to prefitler
/// and sort all matches. All `.._indices` functions will compute the indices of
/// the computed chars. These should be used when rendering the best N matches.
/// Note that the `indices` argument is **never cleared**. This allows running
/// multiple different matches on the same haystack and merging the indices by
/// sorting and deduplicating the vector.
///
/// Matching is limited to 2^32-1 codepoints, if the haystack is longer than
/// that the matcher *will panic*. The caller must decide whether it wants to
/// filter out long haystacks or truncate them.
pub struct Matcher {
    pub config: MatcherConfig,
    slab: MatrixSlab,
}

// this is just here for convenience not ruse if we should implement this
impl Clone for Matcher {
    fn clone(&self) -> Self {
        Matcher {
            config: self.config,
            slab: MatrixSlab::new(),
        }
    }
}

impl std::fmt::Debug for Matcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matcher")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Default for Matcher {
    fn default() -> Self {
        Matcher {
            config: MatcherConfig::DEFAULT,
            slab: MatrixSlab::new(),
        }
    }
}

impl Matcher {
    pub fn new(config: MatcherConfig) -> Self {
        Self {
            config,
            slab: MatrixSlab::new(),
        }
    }

    /// Find the fuzzy match with the higehest score in the `haystack`.
    ///
    /// This functions has `O(mn)` time complexity for short inputs. To
    /// avoid slowdowns it automatically falls back to [greedy matching]
    /// (crate::Matcher::fuzzy_match_greedy) for large needles and haystacks
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn fuzzy_match(&mut self, haystack: Utf32Str<'_>, needle: Utf32Str<'_>) -> Option<u16> {
        assert!(haystack.len() <= u32::MAX as usize);
        self.fuzzy_matcher_impl::<false>(haystack, needle, &mut Vec::new())
    }

    /// Find the fuzzy match with the higehest score in the `haystack` and
    /// compute its indices.
    ///
    /// This functions has `O(mn)` time complexity for short inputs. To
    /// avoid slowdowns it automatically falls back to [greedy matching]
    /// (crate::Matcher::fuzzy_match_greedy) for large needles and haystacks
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn fuzzy_indices(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        assert!(haystack.len() <= u32::MAX as usize);
        self.fuzzy_matcher_impl::<true>(haystack, needle, indices)
    }

    fn fuzzy_matcher_impl<const INDICES: bool>(
        &mut self,
        haystack_: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if needle_.len() > haystack_.len() || needle_.is_empty() {
            return None;
        }
        if needle_.len() == haystack_.len() {
            return self.exact_match_impl::<INDICES>(
                haystack_,
                needle_,
                0,
                haystack_.len(),
                indices,
            );
        }
        assert!(
            haystack_.len() <= u32::MAX as usize,
            "fuzzy matching is only support for up to 2^32-1 codepoints"
        );
        match (haystack_, needle_) {
            (Utf32Str::Ascii(haystack), Utf32Str::Ascii(needle)) => {
                if let &[needle] = needle {
                    return self.substring_match_1_ascii::<INDICES>(haystack, needle, indices);
                }
                let (start, greedy_end, end) = self.prefilter_ascii(haystack, needle, false)?;
                if needle_.len() == end - start {
                    return Some(self.calculate_score::<INDICES, _, _>(
                        AsciiChar::cast(haystack),
                        AsciiChar::cast(needle),
                        start,
                        greedy_end,
                        indices,
                    ));
                }
                self.fuzzy_match_optimal::<INDICES, AsciiChar, AsciiChar>(
                    AsciiChar::cast(haystack),
                    AsciiChar::cast(needle),
                    start,
                    greedy_end,
                    end,
                    indices,
                )
            }
            (Utf32Str::Ascii(_), Utf32Str::Unicode(_)) => {
                // a purely ascii haystack can never be transformed to match
                // a needle that contains non-ascii chars since we don't allow gaps
                None
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Ascii(needle)) => {
                if let &[needle] = needle {
                    let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                    let res = self.substring_match_1_non_ascii::<INDICES>(
                        haystack,
                        needle as char,
                        start,
                        indices,
                    );
                    return Some(res);
                }
                let (start, end) = self.prefilter_non_ascii(haystack, needle_, false)?;
                if needle_.len() == end - start {
                    return self
                        .exact_match_impl::<INDICES>(haystack_, needle_, start, end, indices);
                }
                self.fuzzy_match_optimal::<INDICES, char, AsciiChar>(
                    haystack,
                    AsciiChar::cast(needle),
                    start,
                    start + 1,
                    end,
                    indices,
                )
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Unicode(needle)) => {
                if let &[needle] = needle {
                    let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                    let res = self
                        .substring_match_1_non_ascii::<INDICES>(haystack, needle, start, indices);
                    return Some(res);
                }
                let (start, end) = self.prefilter_non_ascii(haystack, needle_, false)?;
                if needle_.len() == end - start {
                    return self
                        .exact_match_impl::<INDICES>(haystack_, needle_, start, end, indices);
                }
                self.fuzzy_match_optimal::<INDICES, char, char>(
                    haystack,
                    needle,
                    start,
                    start + 1,
                    end,
                    indices,
                )
            }
        }
    }

    /// Greedly find a fuzzy match in the `haystack`.
    ///
    /// This functions has `O(n)` time complexity but may provide unintutive (non-optimal)
    /// indices and scores. Usually [fuzz_indices](crate::Matcher::fuzzy_indices) should
    /// be preferred.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn fuzzy_match_greedy(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
    ) -> Option<u16> {
        assert!(haystack.len() <= u32::MAX as usize);
        self.fuzzy_match_greedy_impl::<false>(haystack, needle, &mut Vec::new())
    }

    /// Greedly find a fuzzy match in the `haystack` and compute its indices.
    ///
    /// This functions has `O(n)` time complexity but may provide unintutive (non-optimal)
    /// indices and scores. Usually [fuzz_indices](crate::Matcher::fuzzy_indices) should
    /// be preferred.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn fuzzy_indices_greedy(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        assert!(haystack.len() <= u32::MAX as usize);
        self.fuzzy_match_greedy_impl::<true>(haystack, needle, indices)
    }

    fn fuzzy_match_greedy_impl<const INDICES: bool>(
        &mut self,
        haystack: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if needle_.len() > haystack.len() || needle_.is_empty() {
            return None;
        }
        if needle_.len() == haystack.len() {
            return self.exact_match_impl::<INDICES>(haystack, needle_, 0, haystack.len(), indices);
        }
        assert!(
            haystack.len() <= u32::MAX as usize,
            "matching is only support for up to 2^32-1 codepoints"
        );
        match (haystack, needle_) {
            (Utf32Str::Ascii(haystack), Utf32Str::Ascii(needle)) => {
                let (start, greedy_end, _) = self.prefilter_ascii(haystack, needle, true)?;
                if needle_.len() == greedy_end - start {
                    return Some(self.calculate_score::<INDICES, _, _>(
                        AsciiChar::cast(haystack),
                        AsciiChar::cast(needle),
                        start,
                        greedy_end,
                        indices,
                    ));
                }
                self.fuzzy_match_greedy_::<INDICES, AsciiChar, AsciiChar>(
                    AsciiChar::cast(haystack),
                    AsciiChar::cast(needle),
                    start,
                    greedy_end,
                    indices,
                )
            }
            (Utf32Str::Ascii(_), Utf32Str::Unicode(_)) => {
                // a purely ascii haystack can never be transformed to match
                // a needle that contains non-ascii chars since we don't allow gaps
                None
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Ascii(needle)) => {
                let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                self.fuzzy_match_greedy_::<INDICES, char, AsciiChar>(
                    haystack,
                    AsciiChar::cast(needle),
                    start,
                    start + 1,
                    indices,
                )
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Unicode(needle)) => {
                let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                self.fuzzy_match_greedy_::<INDICES, char, char>(
                    haystack,
                    needle,
                    start,
                    start + 1,
                    indices,
                )
            }
        }
    }

    /// Finds the substring match with the highest score in the `haystack`.
    ///
    /// This functions has `O(nm)` time complexity. However many cases can
    /// be significantly accelerated using prefilters so it's usually fast
    /// in practice.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn substring_match(
        &mut self,
        haystack: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
    ) -> Option<u16> {
        self.substring_match_impl::<false>(haystack, needle_, &mut Vec::new())
    }

    /// Finds the substring match with the highest score in the `haystack` and
    /// compute its indices.
    ///
    /// This functions has `O(nm)` time complexity. However many cases can
    /// be significantly accelerated using prefilters so it's usually fast
    /// in practice.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn substring_indices(
        &mut self,
        haystack: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        self.substring_match_impl::<true>(haystack, needle_, indices)
    }

    fn substring_match_impl<const INDICES: bool>(
        &mut self,
        haystack: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if needle_.len() > haystack.len() || needle_.is_empty() {
            return None;
        }
        if needle_.len() == haystack.len() {
            return self.exact_match_impl::<INDICES>(haystack, needle_, 0, haystack.len(), indices);
        }
        assert!(
            haystack.len() <= u32::MAX as usize,
            "matching is only support for up to 2^32-1 codepoints"
        );
        match (haystack, needle_) {
            (Utf32Str::Ascii(haystack), Utf32Str::Ascii(needle)) => {
                if let &[needle] = needle {
                    return self.substring_match_1_ascii::<INDICES>(haystack, needle, indices);
                }
                self.substring_match_ascii::<INDICES>(haystack, needle, indices)
            }
            (Utf32Str::Ascii(_), Utf32Str::Unicode(_)) => {
                // a purely ascii haystack can never be transformed to match
                // a needle that contains non-ascii chars since we don't allow gaps
                None
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Ascii(needle)) => {
                if let &[needle] = needle {
                    let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                    let res = self.substring_match_1_non_ascii::<INDICES>(
                        haystack,
                        needle as char,
                        start,
                        indices,
                    );
                    return Some(res);
                }
                let (start, _) = self.prefilter_non_ascii(haystack, needle_, false)?;
                self.substring_match_non_ascii::<INDICES, _>(
                    haystack,
                    AsciiChar::cast(needle),
                    start,
                    indices,
                )
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Unicode(needle)) => {
                if let &[needle] = needle {
                    let (start, _) = self.prefilter_non_ascii(haystack, needle_, true)?;
                    let res = self
                        .substring_match_1_non_ascii::<INDICES>(haystack, needle, start, indices);
                    return Some(res);
                }
                let (start, end) = self.prefilter_non_ascii(haystack, needle_, false)?;
                self.fuzzy_match_optimal::<INDICES, char, char>(
                    haystack,
                    needle,
                    start,
                    start + 1,
                    end,
                    indices,
                )
            }
        }
    }

    /// Checks whether needle and haystack match exactly.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn exact_match(&mut self, haystack: Utf32Str<'_>, needle: Utf32Str<'_>) -> Option<u16> {
        self.exact_match_impl::<false>(haystack, needle, 0, haystack.len(), &mut Vec::new())
    }

    /// Checks whether needle and haystack match exactly and compute the matches indices.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn exact_indices(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        self.exact_match_impl::<true>(haystack, needle, 0, haystack.len(), indices)
    }

    /// Checks whether needle is a prefix of the haystack.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn prefix_match(&mut self, haystack: Utf32Str<'_>, needle: Utf32Str<'_>) -> Option<u16> {
        if haystack.len() < needle.len() {
            None
        } else {
            self.exact_match_impl::<false>(haystack, needle, 0, needle.len(), &mut Vec::new())
        }
    }

    /// Checks whether needle is a prefix of the haystack and compute the matches indices.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn prefix_indices(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if haystack.len() < needle.len() {
            None
        } else {
            self.exact_match_impl::<true>(haystack, needle, 0, needle.len(), indices)
        }
    }

    /// Checks whether needle is a postfix of the haystack.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn postfix_match(&mut self, haystack: Utf32Str<'_>, needle: Utf32Str<'_>) -> Option<u16> {
        if haystack.len() < needle.len() {
            None
        } else {
            self.exact_match_impl::<false>(
                haystack,
                needle,
                haystack.len() - needle.len(),
                haystack.len(),
                &mut Vec::new(),
            )
        }
    }

    /// Checks whether needle is a postfix of the haystack and compute the matches indices.
    ///
    /// This functions has `O(n)` time complexity.
    ///
    /// See the [matcher documentation](crate::Matcher) for more details.
    pub fn postfix_indices(
        &mut self,
        haystack: Utf32Str<'_>,
        needle: Utf32Str<'_>,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if haystack.len() < needle.len() {
            None
        } else {
            self.exact_match_impl::<true>(
                haystack,
                needle,
                haystack.len() - needle.len(),
                haystack.len(),
                indices,
            )
        }
    }

    fn exact_match_impl<const INDICES: bool>(
        &mut self,
        haystack: Utf32Str<'_>,
        needle_: Utf32Str<'_>,
        start: usize,
        end: usize,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        if needle_.len() != end - start || needle_.is_empty() {
            return None;
        }
        assert!(
            haystack.len() <= u32::MAX as usize,
            "matching is only support for up to 2^32-1 codepoints"
        );
        let score = match (haystack, needle_) {
            (Utf32Str::Ascii(haystack), Utf32Str::Ascii(needle)) => {
                let matched = if self.config.ignore_case {
                    AsciiChar::cast(haystack)[start..end]
                        .iter()
                        .map(|c| c.normalize(&self.config))
                        .eq(AsciiChar::cast(needle)
                            .iter()
                            .map(|c| c.normalize(&self.config)))
                } else {
                    haystack == needle
                };
                if !matched {
                    return None;
                }
                self.calculate_score::<INDICES, _, _>(
                    AsciiChar::cast(haystack),
                    AsciiChar::cast(needle),
                    start,
                    end,
                    indices,
                )
            }
            (Utf32Str::Ascii(_), Utf32Str::Unicode(_)) => {
                // a purely ascii haystack can never be transformed to match
                // a needle that contains non-ascii chars since we don't allow gaps
                return None;
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Ascii(needle)) => {
                let matched = haystack[start..end]
                    .iter()
                    .map(|c| c.normalize(&self.config))
                    .eq(AsciiChar::cast(needle)
                        .iter()
                        .map(|c| c.normalize(&self.config)));
                if !matched {
                    return None;
                }

                self.calculate_score::<INDICES, _, _>(
                    haystack,
                    AsciiChar::cast(needle),
                    start,
                    end,
                    indices,
                )
            }
            (Utf32Str::Unicode(haystack), Utf32Str::Unicode(needle)) => {
                let matched = haystack[start..end]
                    .iter()
                    .map(|c| c.normalize(&self.config))
                    .eq(needle.iter().map(|c| c.normalize(&self.config)));
                if !matched {
                    return None;
                }
                self.calculate_score::<INDICES, _, _>(haystack, needle, start, end, indices)
            }
        };
        Some(score)
    }
}
