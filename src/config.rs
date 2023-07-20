use crate::chars::CharClass;
use crate::score::BONUS_BOUNDARY;

#[non_exhaustive]
pub struct MatcherConfig {
    pub delimeter_chars: &'static [u8],
    /// Extra bonus for word boundary after whitespace character or beginning of the string
    pub bonus_boundary_white: u16,

    // Extra bonus for word boundary after slash, colon, semi-colon, and comma
    pub bonus_boundary_delimiter: u16,
    pub inital_char_class: CharClass,
    /// Whether to normalize latin script charaters to ASCII
    /// this significantly degrades performance so its not recommended
    /// to be truned on by default
    pub normalize: bool,
    /// whether to ignore casing
    pub ignore_case: bool,
}

// #[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Copy, Clone, Hash)]
// #[non_exhaustive]
// pub enum CaseMatching {
//     Respect,
//     Ignore,
//     Smart,
// }

impl MatcherConfig {
    pub const DEFAULT: Self = {
        MatcherConfig {
            delimeter_chars: b"/,:;|",
            bonus_boundary_white: BONUS_BOUNDARY + 2,
            bonus_boundary_delimiter: BONUS_BOUNDARY + 1,
            inital_char_class: CharClass::Whitespace,
            normalize: false,
            ignore_case: true,
        }
    };
}

impl MatcherConfig {
    pub fn set_match_paths(&mut self) {
        if cfg!(windows) {
            self.delimeter_chars = b"/\\";
        } else {
            self.delimeter_chars = b"/";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.inital_char_class = CharClass::Delimiter;
    }

    pub const fn match_paths(mut self) -> Self {
        if cfg!(windows) {
            self.delimeter_chars = b"/\\";
        } else {
            self.delimeter_chars = b"/";
        }
        self.bonus_boundary_white = BONUS_BOUNDARY;
        self.inital_char_class = CharClass::Delimiter;
        self
    }
}
