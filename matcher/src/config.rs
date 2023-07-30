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
        }
    };
}

impl MatcherConfig {
    pub fn set_match_paths(&mut self) {
        if cfg!(windows) {
            self.delimiter_chars = b"/\\";
        } else {
            self.delimiter_chars = b"/";
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
