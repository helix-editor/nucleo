use crate::chars::CharClass;
use crate::score::BONUS_BOUNDARY;

/// Configuration data that controls how a matcher behaves
#[non_exhaustive]
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Config {
    /// Characters that act as delimiters and provide bonus
    /// for matching the following char
    pub(crate) delimiter_chars: &'static [u8],
    /// Extra bonus for word boundary after whitespace character or beginning of the string
    pub(crate) bonus_boundary_white: u16,
    /// Extra bonus for word boundary after slash, colon, semi-colon, and comma
    pub(crate) bonus_boundary_delimiter: u16,
    pub(crate) initial_char_class: CharClass,

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
    /// Whether to add a bonus for needle characters that also match within the
    /// haystack's last path segment (its basename). The bonus is the matcher's
    /// own score for the needle against that segment alone, so a candidate
    /// whose filename matches is ranked above siblings that only match in a
    /// shared parent directory. Mirrors fzf's `--scheme=path`.
    pub prefer_basename: bool,
}

impl Config {
    /// The default config for nucleo, implemented as a constant since
    /// Default::default can not be called in a const context
    pub const DEFAULT: Self = {
        Config {
            delimiter_chars: b"/,:;|",
            bonus_boundary_white: BONUS_BOUNDARY + 2,
            bonus_boundary_delimiter: BONUS_BOUNDARY + 1,
            initial_char_class: CharClass::Whitespace,
            normalize: true,
            ignore_case: true,
            prefer_prefix: false,
            prefer_basename: false,
        }
    };
}

impl Config {
    /// Configures the matcher with bonuses appropriate for matching file paths.
    pub fn set_match_paths(&mut self) {
        if cfg!(windows) {
            self.delimiter_chars = b"/:\\";
        } else {
            self.delimiter_chars = b"/:";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.initial_char_class = CharClass::Delimiter;
        self.prefer_basename = true;
    }

    /// Configures the matcher with bonuses appropriate for matching file paths.
    pub const fn match_paths(mut self) -> Self {
        if cfg!(windows) {
            self.delimiter_chars = b"/\\";
        } else {
            self.delimiter_chars = b"/";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.initial_char_class = CharClass::Delimiter;
        self.prefer_basename = true;
        self
    }
}
