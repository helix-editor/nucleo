use ::memchr::{memchr, memchr2, memrchr, memrchr2};

use crate::chars::Char;
use crate::utf32_str::Utf32Str;
use crate::Matcher;

#[inline(always)]
fn find_ascii_ignore_case(c: u8, haystack: &[u8]) -> Option<usize> {
    if c >= b'a' || c <= b'z' {
        memchr2(c, c - 32, haystack)
    } else {
        memchr(c, haystack)
    }
}

#[inline(always)]
fn find_ascii_ignore_case_rev(c: u8, haystack: &[u8]) -> Option<usize> {
    if c >= b'a' || c <= b'z' {
        memrchr2(c, c - 32, haystack)
    } else {
        memrchr(c, haystack)
    }
}

impl Matcher {
    pub(crate) fn prefilter_ascii(
        &self,
        mut haystack: &[u8],
        needle: &[u8],
    ) -> Option<(usize, usize, usize)> {
        if self.config.ignore_case {
            let start = find_ascii_ignore_case(needle[0], haystack)?;
            let mut eager_end = start + 1;
            haystack = &haystack[eager_end..];
            for &c in &needle[1..] {
                let idx = find_ascii_ignore_case(c, haystack)? + 1;
                eager_end += idx;
                haystack = &haystack[idx..];
            }
            let end = eager_end
                + find_ascii_ignore_case_rev(*needle.last().unwrap(), haystack).unwrap_or(0);
            Some((start, eager_end, end))
        } else {
            let start = memchr(needle[0], haystack)?;
            let mut eager_end = start + 1;
            haystack = &haystack[eager_end..];
            for &c in &needle[1..] {
                let idx = memchr(c, haystack)? + 1;
                eager_end += idx;
                haystack = &haystack[idx..];
            }
            let end = eager_end + memrchr(*needle.last().unwrap(), haystack).unwrap_or(0);
            Some((start, eager_end, end))
        }
    }

    pub(crate) fn prefilter_non_ascii(
        &self,
        haystack: &[char],
        needle: Utf32Str<'_>,
    ) -> Option<(usize, usize)> {
        let needle_char = needle.get(0);
        let start = haystack
            .iter()
            .position(|c| c.normalize(&self.config) == needle_char)?;
        let needle_char = needle.last();
        let end = haystack[start..]
            .iter()
            .position(|c| c.normalize(&self.config) == needle_char)?;

        Some((start, end))
    }
}
