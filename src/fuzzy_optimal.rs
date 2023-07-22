use std::cmp::max;
use std::mem::take;

use crate::chars::{Char, CharClass};
use crate::matrix::{haystack, rows_mut, Matrix, MatrixCell, MatrixRow};
use crate::score::{
    BONUS_BOUNDARY, BONUS_CONSECUTIVE, BONUS_FIRST_CHAR_MULTIPLIER, PENALTY_GAP_EXTENSION,
    PENALTY_GAP_START, SCORE_MATCH,
};
use crate::{Matcher, MatcherConfig};

impl Matcher {
    pub(crate) fn fuzzy_match_optimal<const INDICES: bool, H: Char + PartialEq<N>, N: Char>(
        &mut self,
        haystack: &[H],
        needle: &[N],
        start: usize,
        greedy_end: usize,
        end: usize,
        indices: &mut Vec<u32>,
    ) -> Option<u16> {
        // construct a matrix (and copy the haystack), the matrix and haystack size are bounded
        // to avoid the slow O(mn) time complexity for large inputs. Furthermore, it allows
        // us to treat needle indices as u16
        let Some(mut matrix) = self.slab.alloc(&haystack[start..end], needle.len()) else {
            return self.fuzzy_match_greedy_::<INDICES, H, N>(
                haystack,
                needle,
                start,
                greedy_end,
                indices,
            );
        };

        let prev_class = start
            .checked_sub(1)
            .map(|i| haystack[i].char_class(&self.config))
            .unwrap_or(self.config.initial_char_class);
        let (max_score_pos, max_score, matched) = matrix.setup(needle, prev_class, &self.config);
        // this only happened with unicode haystacks, for ASCII the prefilter handles all rejects
        if !matched {
            debug_assert!(!(H::ASCII && N::ASCII));
            return None;
        }
        if needle.len() == 1 {
            indices.clear();
            indices.push(max_score_pos as u32 + start as u32);
            return Some(max_score);
        }
        debug_assert_eq!(
            matrix.row_offs[0], 0,
            "prefilter should have put us at the start of the match"
        );

        // populate the matrix and find the best score
        let (max_score, best_match_end) = matrix.populate_matrix(needle);
        if INDICES {
            matrix.reconstruct_optimal_path(needle, start as u32, indices, best_match_end);
        }
        Some(max_score)
    }
}

impl<H: Char> Matrix<'_, H> {
    fn setup<N: Char>(
        &mut self,
        needle: &[N],
        mut prev_class: CharClass,
        config: &MatcherConfig,
    ) -> (u16, u16, bool)
    where
        H: PartialEq<N>,
    {
        let haystack_len = self.haystack.len() as u16;
        let mut row_iter = needle.iter().copied().zip(self.row_offs.iter_mut());
        let (mut needle_char, mut row_start) = row_iter.next().unwrap();

        let col_iter = self
            .haystack
            .iter_mut()
            .zip(self.cells.iter_mut())
            .zip(self.bonus.iter_mut())
            .enumerate();

        let mut max_score = 0;
        let mut max_score_pos = 0;
        let mut in_gap = false;
        let mut prev_score = 0u16;
        let mut matched = false;
        let first_needle_char = needle[0];
        let mut matrix_cells = 0;

        for (i, ((c_, matrix_cell), bonus_)) in col_iter {
            let (c, class) = c_.char_class_and_normalize(config);
            *c_ = c;

            let bonus = config.bonus_for(prev_class, class);
            // save bonus for later so we don't have to recompute it each time
            *bonus_ = bonus;
            prev_class = class;

            let i = i as u16;
            if c == needle_char {
                // save the first idx of each char
                if let Some(next) = row_iter.next() {
                    matrix_cells += haystack_len - i;
                    *row_start = i;
                    (needle_char, row_start) = next;
                } else if !matched {
                    matrix_cells += haystack_len - i;
                    *row_start = i;
                    // we have atleast one match
                    matched = true;
                }
            }

            // we calculate two scores:
            // * one for transversing the matrix horizontially (no match at
            //   the current char)
            // * one for transversing the matrix diagonally (match at the
            //   current char)
            // the maximum of those two scores is used
            let gap_penalty = if in_gap {
                PENALTY_GAP_EXTENSION
            } else {
                PENALTY_GAP_START
            };
            let score_gap = prev_score.saturating_sub(gap_penalty);
            let score_match = SCORE_MATCH + bonus * BONUS_FIRST_CHAR_MULTIPLIER;
            if c == first_needle_char && score_match >= score_gap {
                matrix_cell.consecutive_chars = 1;
                matrix_cell.score = score_match;
                in_gap = false;
                if needle.len() == 1 && score_match > max_score {
                    max_score = score_match;
                    max_score_pos = i;
                    // can't get better than this
                    if bonus >= BONUS_BOUNDARY {
                        break;
                    }
                }
            } else {
                matrix_cell.consecutive_chars = 0;
                matrix_cell.score = score_gap;
                in_gap = true;
            }
            prev_score = matrix_cell.score;
        }
        self.cells = &mut take(&mut self.cells)[..matrix_cells as usize];
        (max_score_pos, max_score, matched)
    }

    fn populate_matrix<N: Char>(&mut self, needle: &[N]) -> (u16, u16)
    where
        H: PartialEq<N>,
    {
        let mut max_score = 0;
        let mut max_score_end = 0;

        let mut row_iter = needle
            .iter()
            .zip(rows_mut(self.row_offs, self.cells, self.haystack.len()))
            .enumerate();
        // skip the first row we already calculated the in `setup` initial scores
        let (_, mut prev_matrix_row) = row_iter.next().unwrap().1;

        for (i, (&needle_char, row)) in row_iter {
            let haystack = haystack(self.haystack, self.bonus, row.off);
            let mut in_gap = false;
            let mut prev_matrix_cell = MatrixCell {
                score: 0,
                consecutive_chars: 0,
            };
            // we are interested in the score of the previous character
            // in the previous row. This represents the previous char
            // for each possible pattern. This is equivalent to diagonal movement
            let diagonal_start = row.off - prev_matrix_row.off - 1;
            let diagonal = &mut prev_matrix_row.cells[diagonal_start as usize..];

            for (j, ((haystack_char, matrix_cell), &diag_matrix_cell)) in haystack
                .zip(row.cells.iter_mut())
                .zip(diagonal.iter())
                .enumerate()
            {
                let col = j + row.off as usize;
                let gap_penalty = if in_gap {
                    PENALTY_GAP_EXTENSION
                } else {
                    PENALTY_GAP_START
                };
                // we calculate two scores:
                // * one for transversing the matrix horizontially (no match at
                //   the current char)
                // * one for transversing the matrix diagonally (match at the
                //   current char)
                // the maximum of those two scores is used
                let mut score_diag = 0;
                let score_hor = prev_matrix_cell.score.saturating_sub(gap_penalty);

                let mut consecutive = 0;
                if haystack_char.char == needle_char {
                    // we have a match at the current char
                    score_diag = diag_matrix_cell.score + SCORE_MATCH;
                    let mut bonus = haystack_char.bonus;
                    consecutive = diag_matrix_cell.consecutive_chars + 1;
                    if consecutive > 1 {
                        let first_bonus = self.bonus[col + 1 - consecutive as usize];
                        if bonus > first_bonus {
                            if bonus >= BONUS_BOUNDARY {
                                consecutive = 1
                            } else {
                                bonus = max(bonus, BONUS_CONSECUTIVE)
                            }
                        } else {
                            bonus = max(first_bonus, BONUS_CONSECUTIVE)
                        }
                    }
                    if score_diag + bonus < score_hor
                        || (consecutive == 1 && score_diag + bonus == score_hor)
                    {
                        score_diag += haystack_char.bonus;
                        consecutive = 0;
                    } else {
                        score_diag += bonus;
                    }
                }
                in_gap = consecutive == 0;
                let score = max(score_diag, score_hor);
                if i == needle.len() - 1 && score > max_score {
                    max_score = score;
                    max_score_end = col as u16;
                }
                matrix_cell.consecutive_chars = consecutive;
                matrix_cell.score = score;
                prev_matrix_cell = *matrix_cell;
            }
            prev_matrix_row = row;
        }
        (max_score, max_score_end)
    }

    fn reconstruct_optimal_path<N: Char>(
        &self,
        needle: &[N],
        start: u32,
        indices: &mut Vec<u32>,
        best_match_end: u16,
    ) {
        indices.clear();
        indices.resize(needle.len(), 0);

        let mut row_iter = self.rows_rev().zip(indices.iter_mut().rev()).peekable();
        let (mut row, mut matched_col_idx) = row_iter.next().unwrap();
        let mut next_row: Option<MatrixRow> = None;
        let mut col = best_match_end;
        let mut prefer_match = true;
        let haystack_len = self.haystack.len() as u16;

        loop {
            let score = row[col].score;
            // we calculate two scores:
            // * one for transversing the matrix horizontially (no match at
            //   the current char)
            // * one for transversing the matrix diagonally (match at the
            //   current char)
            // the maximum of those two scores is used
            let mut score_diag = 0;
            let mut score_horz = 0;
            if let Some(&(prev_row, _)) = row_iter.peek() {
                score_diag = prev_row[col - 1].score;
            }
            if col > row.off {
                score_horz = row[col - 1].score;
            }
            let mut in_block = row[col].consecutive_chars > 1;
            if !in_block && col + 1 < haystack_len {
                if let Some(next_row) = next_row {
                    if col + 1 >= next_row.off {
                        in_block = next_row[col + 1].consecutive_chars > 1
                    }
                }
            }
            if score > score_diag
                && (score > score_horz || in_block || prefer_match && score == score_horz)
            {
                *matched_col_idx = col as u32 + start;
                next_row = Some(row);
                let Some(next) = row_iter.next() else {
                    break;
                };
                (row, matched_col_idx) = next
            }
            col -= 1;
            prefer_match = row[col].consecutive_chars != 0;
        }
    }
}
