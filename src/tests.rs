use cov_mark::check;

use crate::chars::Char;
use crate::score::{
    BONUS_BOUNDARY, BONUS_CAMEL123, BONUS_CONSECUTIVE, BONUS_FIRST_CHAR_MULTIPLIER, BONUS_NON_WORD,
    PENALTY_GAP_EXTENSION, PENALTY_GAP_START, SCORE_MATCH,
};
use crate::utf32_str::Utf32Str;
use crate::{Matcher, MatcherConfig};

use Algorithm::*;

#[derive(Debug)]
enum Algorithm {
    FuzzyOptimal,
    FuzzyGreedy,
}

fn assert_matches(
    algorithm: &[Algorithm],
    normalize: bool,
    case_sensitive: bool,
    path: bool,
    cases: &[(&str, &str, &[u32], u16)],
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
    let mut matched_indices = Vec::new();
    let mut needle_buf = Vec::new();
    let mut haystack_buf = Vec::new();
    for &(haystack, needle, indices, mut score) in cases {
        let needle = if !case_sensitive {
            needle.to_lowercase()
        } else {
            needle.to_owned()
        };
        let needle = Utf32Str::new(&needle, &mut needle_buf);
        let haystack = Utf32Str::new(haystack, &mut haystack_buf);
        score += needle.len() as u16 * SCORE_MATCH;
        for algo in algorithm {
            println!("xx {matched_indices:?} {algo:?}");
            let res = match algo {
                Algorithm::FuzzyOptimal => {
                    matched_indices.clear();
                    matcher.fuzzy_indices(haystack, needle, &mut matched_indices)
                }
                Algorithm::FuzzyGreedy => {
                    matched_indices.clear();
                    matcher.fuzzy_indices_greedy(haystack, needle, &mut matched_indices)
                }
            };
            println!("{matched_indices:?}");
            let match_chars: Vec<_> = matched_indices
                .iter()
                .map(|&i| haystack.get(i).normalize(&matcher.config))
                .collect();
            let needle_chars: Vec<_> = needle.chars().collect();

            assert_eq!(
                res,
                Some(score),
                "{needle:?} did  not match {haystack:?}: matched {match_chars:?} {matched_indices:?} {algo:?}"
            );
            assert_eq!(
                matched_indices, indices,
                "{needle:?} match {haystack:?} {algo:?}"
            );
            assert_eq!(
                match_chars, needle_chars,
                "{needle:?} match {haystack:?} indices are incorrect {matched_indices:?} {algo:?}"
            );
        }
    }
}

pub fn assert_not_matches(
    normalize: bool,
    case_sensitive: bool,
    path: bool,
    cases: &[(&str, &str)],
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
    let mut needle_buf = Vec::new();
    let mut haystack_buf = Vec::new();
    for &(haystack, needle) in cases {
        let needle = if !case_sensitive {
            needle.to_lowercase()
        } else {
            needle.to_owned()
        };
        let needle = Utf32Str::new(&needle, &mut needle_buf);
        let haystack = Utf32Str::new(haystack, &mut haystack_buf);

        let res = matcher.fuzzy_match(haystack, needle);
        assert_eq!(res, None, "{needle:?} should not match {haystack:?}");
        let res = matcher.fuzzy_match_greedy(haystack, needle);
        assert_eq!(
            res, None,
            "{needle:?} should not match {haystack:?} (greedy)"
        )
    }
}
const BONUS_BOUNDARY_WHITE: u16 = MatcherConfig::DEFAULT.bonus_boundary_white;
const BONUS_BOUNDARY_DELIMITER: u16 = MatcherConfig::DEFAULT.bonus_boundary_delimiter;

#[test]
fn test_fuzzy() {
    assert_matches(
        &[FuzzyGreedy, FuzzyOptimal],
        false,
        false,
        false,
        &[
            (
                "fooBarbaz1",
                "oBZ",
                &[2, 3, 8],
                BONUS_CAMEL123 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "foo bar baz",
                "fbb",
                &[0, 4, 8],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "/AutomatorDocument.icns",
                "rdoc",
                &[9, 10, 11, 12],
                BONUS_CAMEL123 * 3,
            ),
            (
                "/man1/zshcompctl.1",
                "zshc",
                &[6, 7, 8, 9],
                BONUS_BOUNDARY_DELIMITER * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_BOUNDARY_DELIMITER * 3,
            ),
            (
                "/.oh-my-zsh/cache",
                "zshc",
                &[8, 9, 10, 12],
                BONUS_BOUNDARY * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY * 2
                    - PENALTY_GAP_START
                    + BONUS_BOUNDARY_DELIMITER,
            ),
            (
                "ab0123 456",
                "12356",
                &[3, 4, 5, 8, 9],
                BONUS_CONSECUTIVE * 3 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION,
            ),
            (
                "abc123 456",
                "12356",
                &[3, 4, 5, 8, 9],
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_CONSECUTIVE
                    - PENALTY_GAP_START
                    - PENALTY_GAP_EXTENSION,
            ),
            (
                "foo/bar/baz",
                "fbb",
                &[0, 4, 8],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_DELIMITER * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "fooBarBaz",
                "fbb",
                &[0, 3, 6],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_CAMEL123 * 2
                    - 2 * PENALTY_GAP_START
                    - 2 * PENALTY_GAP_EXTENSION,
            ),
            (
                "foo barbaz",
                "fbb",
                &[0, 4, 7],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE
                    - PENALTY_GAP_START * 2
                    - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "fooBar Baz",
                "foob",
                &[0, 1, 2, 3],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 3,
            ),
            (
                "xFoo-Bar Baz",
                "foo-b",
                &[1, 2, 3, 4, 5],
                BONUS_CAMEL123 * BONUS_FIRST_CHAR_MULTIPLIER
                    + BONUS_CAMEL123 * 2
                    + BONUS_NON_WORD
                    + BONUS_BOUNDARY,
            ),
            (
                "]\0\0\0H\0\0\0rrrrrrrrrrrrrrrrrrrrrrrVVVVVVVV\0",
                "H\0\0VV",
                &[4, 5, 6, 31, 32],
                BONUS_BOUNDARY * (BONUS_FIRST_CHAR_MULTIPLIER + 2) + 2 * BONUS_CAMEL123
                    - PENALTY_GAP_START
                    - 23 * PENALTY_GAP_EXTENSION,
            ),
            (
                "\nץ&`@ `---\0\0\0\0",
                "`@ `--\0\0",
                &[3, 4, 5, 6, 7, 8, 10, 11],
                BONUS_NON_WORD * (BONUS_FIRST_CHAR_MULTIPLIER + 3) + BONUS_BOUNDARY_WHITE * 4
                    - PENALTY_GAP_START,
            ),
            (
                " 1111111u11111uuu111",
                "11111uuu1",
                &[9, 10, 11, 12, 13, 14, 15, 16, 17],
                BONUS_CAMEL123 * (BONUS_FIRST_CHAR_MULTIPLIER + 8),
            ),
        ],
    );
}

#[test]
fn test_fuzzy_case_sensitive() {
    assert_matches(
        &[FuzzyGreedy, FuzzyOptimal],
        false,
        true,
        false,
        &[
            (
                "fooBarbaz1",
                "oBz",
                &[2, 3, 8],
                BONUS_CAMEL123 - PENALTY_GAP_START - PENALTY_GAP_EXTENSION * 3,
            ),
            (
                "Foo/Bar/Baz",
                "FBB",
                &[0, 4, 8],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_DELIMITER * 2
                    - 2 * PENALTY_GAP_START
                    - 4 * PENALTY_GAP_EXTENSION,
            ),
            (
                "FooBarBaz",
                "FBB",
                &[0, 3, 6],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_CAMEL123 * 2
                    - 2 * PENALTY_GAP_START
                    - 2 * PENALTY_GAP_EXTENSION,
            ),
            (
                "FooBar Baz",
                "FooB",
                &[0, 1, 2, 3],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE * 3,
            ),
            // Consecutive bonus updated
            (
                "foo-bar",
                "o-ba",
                &[2, 3, 4, 5],
                BONUS_BOUNDARY * 2 + BONUS_NON_WORD,
            ),
        ],
    );
}

#[test]
fn test_normalize() {
    assert_matches(
        &[FuzzyGreedy, FuzzyOptimal],
        true,
        false,
        false,
        &[
            (
                "Só Danço Samba",
                "So",
                &[0, 1],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE,
            ),
            (
                "Só Danço Samba",
                "sodc",
                &[0, 1, 3, 6],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE
                    - PENALTY_GAP_START
                    + BONUS_BOUNDARY_WHITE
                    - PENALTY_GAP_START
                    - PENALTY_GAP_EXTENSION,
            ),
            (
                "Danço",
                "danco",
                &[0, 1, 2, 3, 4],
                BONUS_BOUNDARY_WHITE * (BONUS_FIRST_CHAR_MULTIPLIER + 4),
            ),
            (
                "DanÇo",
                "danco",
                &[0, 1, 2, 3, 4],
                BONUS_BOUNDARY_WHITE * (BONUS_FIRST_CHAR_MULTIPLIER + 4),
            ),
            (
                "xÇando",
                "cando",
                &[1, 2, 3, 4, 5],
                BONUS_CAMEL123 * (BONUS_FIRST_CHAR_MULTIPLIER + 4),
            ),
            ("ۂ(GCGɴCG", "n", &[5], 0),
        ],
    )
}

#[test]
fn test_unicode1() {
    assert_matches(
        &[FuzzyGreedy, FuzzyOptimal],
        true,
        false,
        false,
        &[
            (
                "你好世界",
                "你好",
                &[0, 1],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY_WHITE,
            ),
            (
                "你好世界",
                "你世",
                &[0, 2],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER - PENALTY_GAP_START,
            ),
        ],
    )
}

#[test]
fn test_long_str() {
    assert_matches(
        &[FuzzyGreedy, FuzzyOptimal],
        false,
        false,
        false,
        &[(
            &"x".repeat(u16::MAX as usize + 1),
            "xx",
            &[0, 1],
            (BONUS_FIRST_CHAR_MULTIPLIER + 1) * BONUS_BOUNDARY_WHITE,
        )],
    );
}

#[test]
fn test_optimal() {
    assert_matches(
        &[FuzzyOptimal],
        false,
        false,
        false,
        &[
            (
                "axxx xx ",
                "xx",
                &[5, 6],
                (BONUS_FIRST_CHAR_MULTIPLIER + 1) * BONUS_BOUNDARY_WHITE,
            ),
            (
                "I\0I",
                "\0",
                &[1],
                BONUS_FIRST_CHAR_MULTIPLIER * BONUS_NON_WORD,
            ),
            (
                "SS!H",
                "S!",
                &[0, 2],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_NON_WORD
                    - PENALTY_GAP_START,
            ),
            (
                "^^^\u{7f}\0\0E%\u{1a}^",
                "^^\0E",
                &[1, 2, 5, 6],
                BONUS_NON_WORD * (BONUS_FIRST_CHAR_MULTIPLIER + 3)
                    - PENALTY_GAP_START
                    - PENALTY_GAP_EXTENSION,
            ),
            (
                "Hٷ!!-!!!\n--\u{4}\u{c}-\u{8}-!\u{c}",
                "-!--!",
                &[4, 5, 13, 15, 16],
                BONUS_NON_WORD * (BONUS_FIRST_CHAR_MULTIPLIER + 4)
                    - 2 * PENALTY_GAP_START
                    - 6 * PENALTY_GAP_EXTENSION,
            ),
            (
                "C8Gۂ(GECGS",
                "8GCG",
                &[1, 2, 7, 8],
                BONUS_CAMEL123 * (BONUS_FIRST_CHAR_MULTIPLIER + 1)
                    - PENALTY_GAP_START
                    - 3 * PENALTY_GAP_EXTENSION
                    + BONUS_CONSECUTIVE,
            ),
            (
                "\nץ&`@ `;;;\0\0\0\0",
                "`@ `;;\0\0",
                &[3, 4, 5, 6, 7, 9, 10, 11],
                BONUS_NON_WORD * (BONUS_FIRST_CHAR_MULTIPLIER + 1)
                    + BONUS_BOUNDARY_DELIMITER * 3
                    + BONUS_BOUNDARY_WHITE * 3
                    - PENALTY_GAP_START,
            ),
            (
                "dddddd\0\0\0ddddfdddddd",
                "dddddfddddd",
                &[0, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
                BONUS_BOUNDARY_WHITE * BONUS_FIRST_CHAR_MULTIPLIER + BONUS_BOUNDARY * 10
                    - PENALTY_GAP_START
                    - 7 * PENALTY_GAP_EXTENSION,
            ),
        ],
    );
}
// #[test]
// fn test_greedy() {
//     assert_matches(
//         &[FuzzyGreedy],
//         false,
//         false,
//         false,
//         &[
//             ("SS!H", "S!", &[1, 2], BONUS_NON_WORD),
//             (
//                 "]\0\0\0H\0\0\0rrrrrrrrrrrrrrrrrrrrrrrVVVVVVVV\0",
//                 "H\0\0VV",
//                 &[4, 5, 6, 31, 32],
//                 BONUS_BOUNDARY * (BONUS_FIRST_CHAR_MULTIPLIER + 2) + 2 * BONUS_CAMEL123
//                     - PENALTY_GAP_START
//                     - 23 * PENALTY_GAP_EXTENSION,
//             ),
//         ],
//     );
// }

#[test]
fn test_reject() {
    assert_not_matches(
        true,
        false,
        false,
        &[
            ("你好界", "abc"),
            ("你好世界", "富"),
            ("Só Danço Samba", "sox"),
            ("fooBarbaz", "fooBarbazz"),
        ],
    );
    assert_not_matches(
        true,
        true,
        false,
        &[
            ("你好界", "abc"),
            ("abc", "你"),
            ("你好世界", "富"),
            ("Só Danço Samba", "sox"),
            ("fooBarbaz", "oBZ"),
            ("Foo Bar Baz", "fbb"),
            ("fooBarbaz", "fooBarbazz"),
        ],
    );
    assert_not_matches(
        false,
        true,
        false,
        &[("Só Danço Samba", "sod"), ("Só Danço Samba", "soc")],
    );
    check!(small_haystack);
    assert_not_matches(false, false, false, &[("ۂۂfoۂۂ", "foo")]);
}
