# Changelog

# [0.3.1] - 2023-12-22

## Bugfixes

* fix Unicode substring matcher expecting an exact match (rejecting trailing characters)

# [0.3.0] - 2023-12-22

## **Breaking Changes**

* Pattern API method now requires a Unicode `Normalization` strategy in addition to a `CaseMatching` strategy.

## Bugfixes

* avoid incorrect matches when searching for ASCII needles in a Unicode haystack
* correctly handle Unicode normalization when there are normalizable characters in the pattern, for example characters with umlauts
* when the needle is composed of a single char, return the score and index
  of the best position instead of always returning the first matched character
  in the haystack

# [0.2.1] - 2023-09-02

## Bugfixes

* ensure matcher runs on first call to `tick`

# [0.2.0] - 2023-09-01

*initial public release*


[0.3.0]: https://github.com/helix-editor/nucleo/releases/tag/nucleo-v0.3.0
[0.2.1]: https://github.com/helix-editor/nucleo/releases/tag/nucleo-v0.2.1
[0.2.0]: https://github.com/helix-editor/nucleo/releases/tag/nucleo-v0.2.0
