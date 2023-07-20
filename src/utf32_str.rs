use std::ops::{Bound, RangeBounds};
use std::slice;

/// A UTF32 encoded (char array) String that can be used as an input to fuzzy matching.
///
/// Usually rusts utf8 encoded strings are great. However during fuzzy matching
/// operates on codepoints (it should operate on graphemes but that's too much
/// hassle to deal with). We want to quickly iterate these codeboints between
/// (up to 5 times) during matching.
///
/// Doing codepoint segmentation on the fly not only blows trough the cache
/// (lookuptables and Icache) but also has nontrivial runtime compared to the
/// matching itself. Furthermore there are a lot of exta optimizations available
/// for ascii only text (but checking during each match has too much overhead).
///
/// Ofcourse this comes at exta memory cost as we usally still need the ut8
/// encoded variant for rendenring. In the (dominant) case of ascii-only text
/// we don't require a copy. Furthermore fuzzy matching usually is applied while
/// the user is typing on the fly so the same item is potentially matched many
/// times (making the the upfront cost more worth it). That means that its
/// basically always worth it to presegment the string.
///
/// For usecases that only match (a lot of) strings once its possible to keep
/// char buffer around that is filled with the presegmented chars
///
/// Another advantage of this approach is that the matcher will naturally
/// produce char indecies (instead of utf8 offsets) annyway. With a
/// codepoint basec representation like this the indecies can be used
/// directly
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
pub enum Utf32Str<'a> {
    /// A string represented as ASCII encoded bytes.
    /// Correctness invariant: must only contain vaild ASCII (<=127)
    Ascii(&'a [u8]),
    /// A string represented as an array of unicode codepoints (basically UTF-32).
    Unicode(&'a [char]),
}

impl<'a> Utf32Str<'a> {
    /// Convenience method to construct a `Utf32Str` from a normal utf8 str
    pub fn new(str: &'a str, buf: &'a mut Vec<char>) -> Self {
        if str.is_ascii() {
            Utf32Str::Ascii(str.as_bytes())
        } else {
            buf.clear();
            buf.extend(str.chars());
            Utf32Str::Unicode(&*buf)
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Utf32Str::Unicode(codepoints) => codepoints.len(),
            Utf32Str::Ascii(ascii_bytes) => ascii_bytes.len(),
        }
    }

    #[inline]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Utf32Str {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end,
            Bound::Excluded(&end) => end + 1,
            Bound::Unbounded => self.len(),
        };
        match self {
            Utf32Str::Ascii(bytes) => Utf32Str::Ascii(&bytes[start..end]),
            Utf32Str::Unicode(codepoints) => Utf32Str::Unicode(&codepoints[start..end]),
        }
    }

    /// Same as `slice` but accepts a u32 range for convenicene sine
    /// those are the indecies returned by the matcher
    #[inline]
    pub fn slice_u32(&self, range: impl RangeBounds<u32>) -> Utf32Str {
        let start = match range.start_bound() {
            Bound::Included(&start) => start as usize,
            Bound::Excluded(&start) => start as usize + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end as usize,
            Bound::Excluded(&end) => end as usize + 1,
            Bound::Unbounded => self.len(),
        };
        match self {
            Utf32Str::Ascii(bytes) => Utf32Str::Ascii(&bytes[start..end]),
            Utf32Str::Unicode(codepoints) => Utf32Str::Unicode(&codepoints[start..end]),
        }
    }
    pub fn is_ascii(&self) -> bool {
        matches!(self, Utf32Str::Ascii(_))
    }

    pub fn get(&self, idx: u32) -> char {
        match self {
            Utf32Str::Ascii(bytes) => bytes[idx as usize] as char,
            Utf32Str::Unicode(codepoints) => codepoints[idx as usize],
        }
    }
    pub fn last(&self) -> char {
        match self {
            Utf32Str::Ascii(bytes) => bytes[bytes.len()] as char,
            Utf32Str::Unicode(codepoints) => codepoints[codepoints.len()],
        }
    }
    pub fn chars(&self) -> Chars<'_> {
        match self {
            Utf32Str::Ascii(bytes) => Chars::Ascii(bytes.iter()),
            Utf32Str::Unicode(codepoints) => Chars::Unicode(codepoints.iter()),
        }
    }
}

pub enum Chars<'a> {
    Ascii(slice::Iter<'a, u8>),
    Unicode(slice::Iter<'a, char>),
}
impl<'a> Iterator for Chars<'a> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Chars::Ascii(iter) => iter.next().map(|&c| c as char),
            Chars::Unicode(iter) => iter.next().copied(),
        }
    }
}
