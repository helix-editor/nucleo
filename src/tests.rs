use crate::chars::Char;
use crate::score::{
    BONUS_BOUNDARY, BONUS_CAMEL123, BONUS_CONSECUTIVE, BONUS_FIRST_CHAR_MULTIPLIER, BONUS_NON_WORD,
    PENALTY_GAP_EXTENSION, PENALTY_GAP_START, SCORE_MATCH,
};
use crate::utf32_str::Utf32Str;
use crate::{Matcher, MatcherConfig};

pub fn assert_matches(
    use_v1: bool,
    normalize: bool,
    case_sensitive: bool,
    path: bool,
    cases: &[(&str, &str, u32, u32, u16)],
) {
    let mut config = MatcherConfig {
        normalize,
        ignore_case: !case_sensitive,
        ..MatcherConfig::DEFAULT
    };
    if path {
        config.set_match_paths();
    }
    let mut matcher = Matcher::new(config);
    let mut indicies = Vec::new();
    let mut needle_buf = Vec::new();
    let mut haystack_buf = Vec::new();
    for &(haystack, needle, start, end, mut score) in cases {
        let needle = if !case_sensitive {
            needle.to_lowercase()
        } else {
            needle.to_owned()
        };
        let needle = Utf32Str::new(&needle, &mut needle_buf);
        let haystack = Utf32Str::new(haystack, &mut haystack_buf);
        score += needle.len() as u16 * SCORE_MATCH;

        let res = matcher.fuzzy_indicies(haystack, needle, &mut indicies);
        let match_chars: Vec<_> = indicies
            .iter()
            .map(|&i| haystack.get(i).normalize(&matcher.config))
            .collect();
        let needle_chars: Vec<_> = needle.chars().collect();

        assert_eq!(
            res,
            Some(score),
            "{needle:?} did  not match {haystack:?}: {match_chars:?}"
        );
        assert_eq!(match_chars, needle_chars, "match indicies are incorrect");
        assert_eq!(
            indicies.first().copied()..indicies.last().map(|&i| i + 1),
            Some(start)..Some(end),
            "{needle:?} match {haystack:?}[{start}..{end}]"
        );
    }
}
const BONUS_BOUNDARY_WHITE: u16 = MatcherConfig::DEFAULT.bonus_boundary_white;
const BONUS_BOUNDARY_DELIMITER: u16 = MatcherConfig::DEFAULT.bonus_boundary_delimiter;

#[test]
fn test_v2_fuzzy() {
    assert_matches(
        false,
        false,
        false,
        false,
        &[
            (
                "fooBarbaz1",
                "oBZ",
                2,
                9,
                BONUS_CAMEL123 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "foo bar baz",
                "fbb",
                0,
                9,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "/AutomatorDocument.icns",
                "rdoc",
                9,
                13,
                BONUS_CAMEL123 + BONUS_CONSECUTIVE * 2,
            ),
            (
                "/man1/zshcompctl.1",
                "zshc",
                6,
                10,
                BONUS_BOUNDARY_DELIMITER * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_BOUNDARY_DELIMITER * 3,
            ),
            (
                "/.oh-my-zsh/cache",
                "zshc",
                8,
                13,
                BONUS_BOUNDARY * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY * 2
                    - PENALTY_GAP_START
                    + BONUS_BOUNDARY_DELIMITER,
            ),
            (
                "ab0123 456",
                "12356",
                3,
                10,
                BONUS_CONSECUTIVE * 3 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION,
            ),
            (
                "abc123 456",
                "12356",
                3,
                10,
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_CONSECUTIVE
                    - PENALTY_GAP_START
                    - PENALTY_GAP_EXTENSION,
            ),
            (
                "foo/bar/baz",
                "fbb",
                0,
                9,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_DELIMITER * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "fooBarBaz",
                "fbb",
                0,
                7,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_CAMEL123 * 2
                    - 2 * PENALTY_GAP_START
                    - 2 * PENALTY_GAP_EXTENSION,
            ),
            (
                "foo barbaz",
                "fbb",
                0,
                8,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE
                    - PENALTY_GAP_START * 2
                    - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "fooBar Baz",
                "foob",
                0,
                4,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 3,
            ),
            (
                "xFoo-Bar Baz",
                "foo-b",
                1,
                6,
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_NON_WORD
                    + BONUS_BOUNDARY,
            ),
        ],
    );
}

#[test]
fn test_v1_fuzzy() {
    assert_matches(
        true,
        false,
        false,
        false,
        &[
            (
                "fooBarbaz1",
                "oBZ",
                2,
                9,
                BONUS_CAMEL123 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "foo bar baz",
                "fbb",
                0,
                9,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "/AutomatorDocument.icns",
                "rdoc",
                9,
                13,
                BONUS_CAMEL123 + BONUS_CONSECUTIVE * 2,
            ),
            (
                "/man1/zshcompctl.1",
                "zshc",
                6,
                10,
                BONUS_BOUNDARY_DELIMITER * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_BOUNDARY_DELIMITER * 3,
            ),
            (
                "/.oh-my-zsh/cache",
                "zshc",
                8,
                13,
                BONUS_BOUNDARY * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY * 2
                    - PENALTY_GAP_START
                    + BONUS_BOUNDARY_DELIMITER,
            ),
            (
                "ab0123 456",
                "12356",
                3,
                10,
                BONUS_CONSECUTIVE * 3 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION,
            ),
            (
                "abc123 456",
                "12356",
                3,
                10,
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_CONSECUTIVE
                    - PENALTY_GAP_START
                    - PENALTY_GAP_EXTENSION,
            ),
            (
                "foo/bar/baz",
                "fbb",
                0,
                9,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_DELIMITER * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "fooBarBaz",
                "fbb",
                0,
                7,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_CAMEL123 * 2
                    - 2 * PENALTY_GAP_START
                    - 2 * PENALTY_GAP_EXTENSION,
            ),
            (
                "foo barbaz",
                "fbb",
                0,
                8,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE
                    - PENALTY_GAP_START * 2
                    - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "fooBar Baz",
                "foob",
                0,
                4,
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 3,
            ),
            (
                "xFoo-Bar Baz",
                "foo-b",
                1,
                6,
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_NON_WORD
                    + BONUS_BOUNDARY,
            ),
        ],
    );
}
