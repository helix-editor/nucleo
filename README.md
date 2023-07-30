# Nucleo

An optimized rust port of the fzf fuzzy matching algorithm

## Notes:

* [x] fuzzy matcher
 * based on https://www.cs.cmu.edu/~ckingsf/bioinfo-lectures/gaps.pdf
 * compared to theory we don't store the p-matrix at all and instead just store the value in a variable as we iterate the row
 * this is possible because we precompute the m-matrix for the next row. This is super confusing but greatly increases cache locality and halfes the amount of space we need during matching for the m-matrix too
 * during index calculation full `O(mn)` space matrix is required. We only store
   two bools to allow backtracking indices, skim stores the full p and m matrix in that case => doesn't matter too much as indices are only computed for visible elements
 * space complexity: skim needs at least 8x more memory => much worse case locality, fzf always allocates a full `O(mn)` matrix even during matching => byebye cache (atleast they use smaller integers tough)
 * nucleos' matrix only was width `n-m+1` instead of width `n`. This comes from the observation that the `p.` char requires `p-1` chars before it and `m-p` chars after it, so there are always `p-1 + m-p = m+1` chars that can never match the current char. This works especially well with only using a single row because the first relevant char is always at the same position even tough its technically further to the right. This is particularly nice because we precalculate the m -matrix which is computed from diagonal elements, so the precalculated values stay in the same matrix cell. 
 * a couple simpler (but arguably even more impactful) optimizations:
    * we presegment unicode, unicode segmentation is somewhat slow and matcher will filter the same elements quite often so only doing it once is nice. It also prevents a very common source of bugs (mixing of char indices which we use here and utf8 indices) and makes the code a lot simpler as a result.
    * we special case ASCII since 90% of practical text is ASCII. ASCII can be stored as bytes instead of `chars` => much better cache locality => we can use memchar (SIMD!).
    * we aggressively prefilter (especially ASCII but also unicode to a lesser extent) to ensure we reject non-matching haystacks as fast as possible. Usually most haystacks will not match when fuzzy matching large lists so having very quick reject past is good
    * for very long matches we fallback to a greedy matcher which runs in `O(N)` (and `O(1)` space complexity) to avoid the `O(mn)` blowup. This is fzfs old algorithm and yields decent (but not great) results.
  * There is a misunderstanding in both skim and fzf. Basically what they do is give a bonus to each character (like word boundaries). That makes senes and is reasonable, but the problem is that they use the **maximum bonus** when multiple chars match in sequence. That means that the bonus of a character depends on which characters exactly matched around it. But the fundamental assumption of this algorithm (and why it doesn't require backtracking) is that the score of each character is independent of what other chars matched (this is the difference between the affine gap and the generic gap case shown in the paper too). During fuzzing I found many cases where this mechanism leads to a non-optimal match being reported (so the sort order and fuzzy indices would be wrong). In my testing removing this mechanism and slightly tweaking the bonus calculation results in similar match quality but made sure the algorithm always worked correctly (and removed a bunch of weird edges cases). 
* [x] substring/prefix/postfix/exact matcher
* [ ] case mismatch penalty. This doesn't seem like a good idea to me. FZF doesn't do this (only skin), smart case should cover most cases. .would be nice for fully case-insensitive matching without smart case like in autocompletion tough. Realistically there won't be more than 3 items that are identical with different casing tough, so I don't think it matters too much. It is a bit annoying to implement since you can no longer pre-normalize queries(or need two queries) :/
* [ ] high level API (worker thread, query parsing, sorting), in progress
  * apparently sorting is superfast (at most 5% of match time for nucleo matcher with a highly selective query, otherwise its completely negligible compared to fuzzy matching). All the bending over backwards fzf does (and skim copied but way worse) seems a little silly. I think fzf does it because go doesn't have a good parallel sort. Fzf divides the matches into a couple fairly large chunks and sorts those on each worker thread and then lazily merges the result. That makes the sorting without the merging `Nlog(N/M)` which is basically equivalent for large `N` and small `M` as is the case here. Atleast its parallel tough. In rust we have a great pattern defeating parallel quicksort tough (rayon) which is way easier.
  * [x] basic implementation (workers, streaming, invalidation)
  * [ ] verify it actually works
  * [ ] query paring
  * [ ] hook up to helix
  * [ ] currently I simply use a tick system (called on every redraw) 
        together with a redraw/tick nofication (ideally debounced) is that enough?
  * [ ] for streaming callers should buffer their data. Can we provide a better API for that beyond what is currently there?
  * [ ] cleanup code, improve API
  * [ ] write docs

* tests
  * [x] fuzz the fuzzy matcher
  * [x] port the full fzf testsuite for fuzzy matching
  * [ ] port the full skim testsuite for fuzzy matching
  * [ ] highlevel API
  * [ ] test bustring/exact/prefix/postfix match
  * [ ] coverage report (fuzzy matcher was at 86%)
