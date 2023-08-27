use crate::chars::CharClass;
use crate::score::BONUS_BOUNDARY;

#[non_exhaustive]
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct MatcherConfig {
    pub delimiter_chars: &'static [u8],
    /// Extra bonus for word boundary after whitespace character or beginning of the string
    pub(crate) bonus_boundary_white: u16,

    /// Extra bonus for word boundary after slash, colon, semi-colon, and comma
    pub(crate) bonus_boundary_delimiter: u16,
    pub initial_char_class: CharClass,
    /// Whether to normalize latin script characters to ASCII (enabled by default)
    pub normalize: bool,
    /// whether to ignore casing
    pub ignore_case: bool,
    /// Whether to provide a bonus to matches by their distance from the start
    /// of the haystack. The bonus is fairly small compared to the normal gap
    /// penalty to avoid messing with the normal score heuristic. This setting
    /// is not turned on by default and only recommended for autocompletion
    /// usecases where the expectation is that the user is typing the entire
    /// match. For a full fzf-like fuzzy matcher/picker word segmentation and
    /// explicit prefix literals should be used instead.
    pub prefer_prefix: bool,
}

impl MatcherConfig {
    pub const DEFAULT: Self = {
        MatcherConfig {
            delimiter_chars: b"/,:;|",
            bonus_boundary_white: BONUS_BOUNDARY + 2,
            bonus_boundary_delimiter: BONUS_BOUNDARY + 1,
            initial_char_class: CharClass::Whitespace,
            normalize: true,
            ignore_case: true,
            prefer_prefix: false,
        }
    };
}

impl MatcherConfig {
    pub fn set_match_paths(&mut self) {
        // compared to fzf we include
        if cfg!(windows) {
            self.delimiter_chars = b"/:\\";
        } else {
            self.delimiter_chars = b"/:";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.initial_char_class = CharClass::Delimiter;
    }

    pub const fn match_paths(mut self) -> Self {
        if cfg!(windows) {
            self.delimiter_chars = b"/\\";
        } else {
            self.delimiter_chars = b"/";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.initial_char_class = CharClass::Delimiter;
        self
    }
}
