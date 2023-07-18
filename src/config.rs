pub(crate) const SCORE_MATCH: u16 = 16;
pub(crate) const PENALTY_GAP_START: u16 = 3;
pub(crate) const PENALTY_GAP_EXTENSION: u16 = 1;

// We prefer matches at the beginning of a word, but the bonus should not be
// too great to prevent the longer acronym matches from always winning over
// shorter fuzzy matches. The bonus point here was specifically chosen that
// the bonus is cancelled when the gap between the acronyms grows over
// 8 characters, which is approximately the average length of the words found
// in web2 dictionary and my file system.
pub(crate) const BONUS_BOUNDARY: u16 = SCORE_MATCH / 2;

// Although bonus point for non-word characters is non-contextual, we need it
// for computing bonus points for consecutive chunks starting with a non-word
// character.
pub(crate) const BONUS_NON_WORD: u16 = SCORE_MATCH / 2;

// Edge-triggered bonus for matches in camelCase words.
// Compared to word-boundary case, they don't accompany single-character gaps
// (e.g. FooBar vs. foo-bar), so we deduct bonus point accordingly.
pub(crate) const BONUS_CAMEL123: u16 = BONUS_BOUNDARY - PENALTY_GAP_EXTENSION;

// Minimum bonus point given to characters in consecutive chunks.
// Note that bonus points for consecutive matches shouldn't have needed if we
// used fixed match score as in the original algorithm.
pub(crate) const BONUS_CONSECUTIVE: u16 = PENALTY_GAP_START + PENALTY_GAP_EXTENSION;

// The first character in the typed pattern usually has more significance
// than the rest so it's important that it appears at special positions where
// bonus points are given, e.g. "to-go" vs. "ongoing" on "og" or on "ogo".
// The amount of the extra bonus should be limited so that the gap penalty is
// still respected.
pub(crate) const BONUS_FIRST_CHAR_MULTIPLIER: u16 = 2;

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
    /// use faster/simpler algorithm at the cost of (potentially) much worse results
    /// For long inputs this algorith is always used as a fallbach to avoid
    /// blowups in time complexity
    pub use_v1: bool,
    /// The case matching to perform
    pub case_matching: CaseMatching,
}

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Copy, Clone, Hash)]
#[non_exhaustive]
pub enum CharClass {
    Whitespace,
    NonWord,
    Delimiter,
    Lower,
    Upper,
    Letter,
    Number,
}

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Copy, Clone, Hash)]
#[non_exhaustive]
pub enum CaseMatching {
    Respect,
    Ignore,
    Smart,
}

impl MatcherConfig {
    pub const DEFAULT: Self = {
        MatcherConfig {
            delimeter_chars: b"/,:;|",
            bonus_boundary_white: BONUS_BOUNDARY + 2,
            bonus_boundary_delimiter: BONUS_BOUNDARY + 1,
            inital_char_class: CharClass::Whitespace,
            normalize: false,
            use_v1: false,
            case_matching: CaseMatching::Smart,
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

    fn char_class_non_ascii(c: char) -> CharClass {
        if c.is_lowercase() {
            CharClass::Lower
        } else if c.is_uppercase() {
            CharClass::Upper
        } else if c.is_numeric() {
            CharClass::Number
        } else if c.is_alphabetic() {
            CharClass::Letter
        } else if c.is_whitespace() {
            CharClass::Whitespace
        } else {
            CharClass::NonWord
        }
    }

    fn char_class_ascii(&self, c: char) -> CharClass {
        // using manual if conditions instead optimizes better
        if c >= 'a' && c <= 'z' {
            CharClass::Lower
        } else if c >= 'A' && c <= 'Z' {
            CharClass::Upper
        } else if c >= '0' && c <= '9' {
            CharClass::Number
        } else if c.is_ascii_whitespace() {
            CharClass::Whitespace
        } else if self.delimeter_chars.contains(&(c as u8)) {
            CharClass::Delimiter
        } else {
            CharClass::NonWord
        }
    }

    pub(crate) fn char_class(&self, c: char) -> CharClass {
        if c.is_ascii() {
            self.char_class_ascii(c)
        } else {
            Self::char_class_non_ascii(c)
        }
    }

    pub(crate) fn bonus_for(&self, prev_class: CharClass, class: CharClass) -> u16 {
        if class > CharClass::NonWord {
            // transition from non word to word
            match prev_class {
                CharClass::Whitespace => return self.bonus_boundary_white,
                CharClass::Delimiter => return self.bonus_boundary_delimiter,
                CharClass::NonWord => return BONUS_BOUNDARY,
                _ => (),
            }
        }
        if prev_class == CharClass::Lower && class == CharClass::Upper
            || prev_class != CharClass::Number && class == CharClass::Number
        {
            // camelCase letter123
            BONUS_CAMEL123
        } else if class == CharClass::NonWord {
            BONUS_NON_WORD
        } else if class == CharClass::Whitespace {
            self.bonus_boundary_white
        } else {
            0
        }
    }
}
