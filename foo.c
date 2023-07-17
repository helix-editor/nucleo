
fzf_result_t fzf_fuzzy_match_v2(bool case_sensitive, bool normalize,
                                fzf_string_t *text, fzf_string_t *pattern,
                                fzf_position_t *pos, fzf_slab_t *slab) {
  const size_t M = pattern->size;
  const size_t N = text->size;
  if (M == 0) {
    return (fzf_result_t){0, 0, 0};
  }
  if (slab != NULL && N * M > slab->I16.cap) {
    return fzf_fuzzy_match_v1(case_sensitive, normalize, text, pattern, pos,
                              slab);
  }

  size_t idx;
  {
    int32_t tmp_idx = ascii_fuzzy_index(text, pattern->data, M, case_sensitive);
    if (tmp_idx < 0) {
      return (fzf_result_t){-1, -1, 0};
    }
    idx = (size_t)tmp_idx;
  }

  size_t offset16 = 0;
  size_t offset32 = 0;

  fzf_i16_t h0 = alloc16(&offset16, slab, N);
  fzf_i16_t c0 = alloc16(&offset16, slab, N);
  // Bonus point for each positions
  fzf_i16_t bo = alloc16(&offset16, slab, N);
  // The first occurrence of each character in the pattern
  fzf_i32_t f = alloc32(&offset32, slab, M);
  // Rune array
  fzf_i32_t t = alloc32(&offset32, slab, N);
  copy_runes(text, &t); // input.CopyRunes(T)

  // Phase 2. Calculate bonus for each point
  int16_t max_score = 0;
  size_t max_score_pos = 0;

  size_t pidx = 0;
  size_t last_idx = 0;

  char pchar0 = pattern->data[0];
  char pchar = pattern->data[0];
  int16_t prev_h0 = 0;
  int32_t prev_class = CharNonWord;
  bool in_gap = false;

  i32_slice_t t_sub = slice_i32(t.data, idx, t.size); // T[idx:];
  i16_slice_t h0_sub =
      slice_i16_right(slice_i16(h0.data, idx, h0.size).data, t_sub.size);
  i16_slice_t c0_sub =
      slice_i16_right(slice_i16(c0.data, idx, c0.size).data, t_sub.size);
  i16_slice_t b_sub =
      slice_i16_right(slice_i16(bo.data, idx, bo.size).data, t_sub.size);

  for (size_t off = 0; off < t_sub.size; off++) {
    char_class class;
    char c = (char)t_sub.data[off];
    class = char_class_of_ascii(c);
    if (!case_sensitive && class == CharUpper) {
      /* TODO(conni2461): unicode support */
      c = (char)tolower((uint8_t)c);
    }
    if (normalize) {
      c = normalize_rune(c);
    }

    t_sub.data[off] = (uint8_t)c;
    int16_t bonus = bonus_for(prev_class, class);
    b_sub.data[off] = bonus;
    prev_class = class;
    if (c == pchar) {
      if (pidx < M) {
        f.data[pidx] = (int32_t)(idx + off);
        pidx++;
        pchar = pattern->data[min64u(pidx, M - 1)];
      }
      last_idx = idx + off;
    }

    if (c == pchar0) {
      int16_t score = ScoreMatch + bonus * BonusFirstCharMultiplier;
      h0_sub.data[off] = score;
      c0_sub.data[off] = 1;
      if (M == 1 && (score > max_score)) {
        max_score = score;
        max_score_pos = idx + off;
        if (bonus == BonusBoundary) {
          break;
        }
      }
      in_gap = false;
    } else {
      if (in_gap) {
        h0_sub.data[off] = max16(prev_h0 + ScoreGapExtention, 0);
      } else {
        h0_sub.data[off] = max16(prev_h0 + ScoreGapStart, 0);
      }
      c0_sub.data[off] = 0;
      in_gap = true;
    }
    prev_h0 = h0_sub.data[off];
  }
  if (pidx != M) {
    free_alloc(t);
    free_alloc(f);
    free_alloc(bo);
    free_alloc(c0);
    free_alloc(h0);
    return (fzf_result_t){-1, -1, 0};
  }
  if (M == 1) {
    free_alloc(t);
    free_alloc(f);
    free_alloc(bo);
    free_alloc(c0);
    free_alloc(h0);
    fzf_result_t res = {(int32_t)max_score_pos, (int32_t)max_score_pos + 1,
                        max_score};
    append_pos(pos, max_score_pos);
    return res;
  }

  size_t f0 = (size_t)f.data[0];
  size_t width = last_idx - f0 + 1;
  fzf_i16_t h = alloc16(&offset16, slab, width * M);
  {
    i16_slice_t h0_tmp_slice = slice_i16(h0.data, f0, last_idx + 1);
    copy_into_i16(&h0_tmp_slice, &h);
  }

  fzf_i16_t c = alloc16(&offset16, slab, width * M);
  {
    i16_slice_t c0_tmp_slice = slice_i16(c0.data, f0, last_idx + 1);
    copy_into_i16(&c0_tmp_slice, &c);
  }

  i32_slice_t f_sub = slice_i32(f.data, 1, f.size);
  str_slice_t p_sub =
      slice_str_right(slice_str(pattern->data, 1, M).data, f_sub.size);
  for (size_t off = 0; off < f_sub.size; off++) {
    size_t f = (size_t)f_sub.data[off];
    pchar = p_sub.data[off];
    pidx = off + 1;
    size_t row = pidx * width;
    in_gap = false;
    t_sub = slice_i32(t.data, f, last_idx + 1);
    b_sub = slice_i16_right(slice_i16(bo.data, f, bo.size).data, t_sub.size);
    i16_slice_t c_sub = slice_i16_right(
        slice_i16(c.data, row + f - f0, c.size).data, t_sub.size);
    i16_slice_t c_diag = slice_i16_right(
        slice_i16(c.data, row + f - f0 - 1 - width, c.size).data, t_sub.size);
    i16_slice_t h_sub = slice_i16_right(
        slice_i16(h.data, row + f - f0, h.size).data, t_sub.size);
    i16_slice_t h_diag = slice_i16_right(
        slice_i16(h.data, row + f - f0 - 1 - width, h.size).data, t_sub.size);
    i16_slice_t h_left = slice_i16_right(
        slice_i16(h.data, row + f - f0 - 1, h.size).data, t_sub.size);
    h_left.data[0] = 0;
    for (size_t j = 0; j < t_sub.size; j++) {
      char ch = (char)t_sub.data[j];
      size_t col = j + f;
      int16_t s1 = 0;
      int16_t s2 = 0;
      int16_t consecutive = 0;

      if (in_gap) {
        s2 = h_left.data[j] + ScoreGapExtention;
      } else {
        s2 = h_left.data[j] + ScoreGapStart;
      }

      if (pchar == ch) {
        s1 = h_diag.data[j] + ScoreMatch;
        int16_t b = b_sub.data[j];
        consecutive = c_diag.data[j] + 1;
        if (b == BonusBoundary) {
          consecutive = 1;
        } else if (consecutive > 1) {
          b = max16(b, max16(BonusConsecutive,
                             bo.data[col - ((size_t)consecutive) + 1]));
        }
        if (s1 + b < s2) {
          s1 += b_sub.data[j];
          consecutive = 0;
        } else {
          s1 += b;
        }
      }
      c_sub.data[j] = consecutive;
      in_gap = s1 < s2;
      int16_t score = max16(max16(s1, s2), 0);
      if (pidx == M - 1 && (score > max_score)) {
        max_score = score;
        max_score_pos = col;
      }
      h_sub.data[j] = score;
    }
  }

  resize_pos(pos, M, M);
  size_t j = max_score_pos;
  if (pos) {
    size_t i = M - 1;
    bool prefer_match = true;
    for (;;) {
      size_t ii = i * width;
      size_t j0 = j - f0;
      int16_t s = h.data[ii + j0];

      int16_t s1 = 0;
      int16_t s2 = 0;
      if (i > 0 && j >= f.data[i]) {
        s1 = h.data[ii - width + j0 - 1];
      }
      if (j > f.data[i]) {
        s2 = h.data[ii + j0 - 1];
      }

      if (s > s1 && (s > s2 || (s == s2 && prefer_match))) {
        unsafe_append_pos(pos, j);
        if (i == 0) {
          break;
        }
        i--;
      }
      prefer_match = c.data[ii + j0] > 1 || (ii + width + j0 + 1 < c.size &&
                                             c.data[ii + width + j0 + 1] > 0);
      j--;
    }
  }

  free_alloc(h);
  free_alloc(c);
  free_alloc(t);
  free_alloc(f);
  free_alloc(bo);
  free_alloc(c0);
  free_alloc(h0);
  return (fzf_result_t){(int32_t)j, (int32_t)max_score_pos + 1,
                        (int32_t)max_score};
}

