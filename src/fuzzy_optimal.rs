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
    pub(crate) fn fuzzy_match_optimal<const INDICIES: bool, H: Char + PartialEq<N>, N: Char>(
        &mut self,
        haystack: &[H],
        needle: &[N],
        start: usize,
        greedy_end: usize,
        end: usize,
        indicies: &mut Vec<u32>,
    ) -> Option<u16> {
        // construct a matrix (and copy the haystack), the matrix and haystack size are bounded
        // to avoid the slow O(mn) time complexity for large inputs. Furthermore, it allows
        // us to treat needle indecies as u16
        let Some(mut matrix) = self.slab.alloc(&haystack[start..end], needle.len()) else {
            return self.fuzzy_match_greedy::<INDICIES, H, N>(
                haystack,
                needle,
                start,
                greedy_end,
                indicies,
            );
        };

        let prev_class = start
            .checked_sub(1)
            .map(|i| haystack[i].char_class(&self.config))
            .unwrap_or(self.config.inital_char_class);
        let (max_score_pos, max_score, matched) = matrix.setup(needle, prev_class, &self.config);
        // this only happend with unicode haystacks, for ASCII the prefilter handles all rejects
        if !matched {
            return None;
        }
        if needle.len() == 1 {
            indicies.push(max_score_pos as u32);
            return Some(max_score);
        }
        debug_assert_eq!(
            matrix.row_offs[0], 0,
            "prefilter should have put us at the start of the match"
        );

        // populate the matrix and find the best score
        let (max_score, best_match_end) = matrix.populate_matrix(needle);
        if INDICIES {
            matrix.reconstruct_optimal_path(needle, start as u32, indicies, best_match_end);
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

        for (i, ((c, matrix_cell), bonus_)) in col_iter {
            let class = c.char_class(config);
            *c = c.normalize(config);

            let bonus = config.bonus_for(prev_class, class);
            // save bonus for later so we don't have to recompute it each time
            *bonus_ = bonus;
            prev_class = class;

            let i = i as u16;
            if *c == needle_char {
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
            if *c == first_needle_char {
                let score = SCORE_MATCH + bonus * BONUS_FIRST_CHAR_MULTIPLIER;
                matrix_cell.consecutive_chars = 1;
                if needle.len() == 1 && score > max_score {
                    max_score = score;
                    max_score_pos = i;
                    // can't get better than this
                    if bonus >= BONUS_BOUNDARY {
                        break;
                    }
                }
                matrix_cell.score = score;
                in_gap = false;
            } else {
                let gap_penalty = if in_gap {
                    PENALTY_GAP_EXTENSION
                } else {
                    PENALTY_GAP_START
                };
                matrix_cell.score = prev_score.saturating_sub(gap_penalty);
                matrix_cell.consecutive_chars = 0;
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
                let mut score1 = 0;
                let score2 = prev_matrix_cell.score.saturating_sub(gap_penalty);

                let mut consecutive = 0;
                if haystack_char.char == needle_char {
                    score1 = diag_matrix_cell.score + SCORE_MATCH;
                    let mut bonus = haystack_char.bonus;
                    consecutive = diag_matrix_cell.consecutive_chars + 1;
                    if consecutive > 1 {
                        let first_bonus = self.bonus[col + 1 - consecutive as usize];
                        if bonus > first_bonus {
                            if bonus > BONUS_BOUNDARY {
                                consecutive = 1
                            } else {
                                bonus = max(bonus, BONUS_CONSECUTIVE)
                            }
                        } else {
                            bonus = max(first_bonus, BONUS_CONSECUTIVE)
                        }
                    }
                    if score1 + bonus < score2 {
                        score1 += haystack_char.bonus;
                        consecutive = 0;
                    } else {
                        score1 += bonus;
                    }
                }
                in_gap = score1 < score2;
                let score = max(score1, score2);
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
        indicies: &mut Vec<u32>,
        best_match_end: u16,
    ) {
        indicies.resize(needle.len(), 0);

        let mut row_iter = self.rows_rev().zip(indicies.iter_mut().rev()).peekable();
        let (mut row, mut matched_col_idx) = row_iter.next().unwrap();
        let mut next_row: Option<MatrixRow> = None;
        let mut col = best_match_end;
        let mut prefer_match = true;
        let haystack_len = self.haystack.len() as u16;

        loop {
            let score = row[col].score;
            let mut score1 = 0;
            let mut score2 = 0;
            if let Some(&(prev_row, _)) = row_iter.peek() {
                if col >= prev_row.off {
                    score1 = prev_row[col].score;
                }
            }
            if col > row.off {
                score2 = row[col - 1].score;
            }
            let mut new_prefer_match = row[col].consecutive_chars > 1;
            if !new_prefer_match && col + 1 < haystack_len {
                if let Some(next_row) = next_row {
                    if col + 1 > next_row.off {
                        new_prefer_match = next_row[col + 1].consecutive_chars > 0
                    }
                }
            }
            if score > score1 && (score > score2 || score == score2 && prefer_match) {
                *matched_col_idx = col as u32 + start;
                next_row = Some(row);
                let Some(next) = row_iter.next() else {
                    break;
                };
                (row, matched_col_idx) = next
            }
            prefer_match = new_prefer_match;
            col -= 1;
        }
    }
}
