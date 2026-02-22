# Changelog

## [0.3.0](https://github.com/procoperr/janice/compare/v0.2.0...v0.3.0) (2026-02-22)


### Features

* add atomic file sync with crash recovery and --verify flag ([9482cca](https://github.com/procoperr/janice/commit/9482cca25b62b93450220bfac18c3be12d1be9e1))
* add atomic file sync with crash recovery and --verify flag ([faff275](https://github.com/procoperr/janice/commit/faff275669773ed2d4d8ae7396101ccde0519931))


### Bug Fixes

* gate directory fsync to unix (Windows does not support File::open on dirs) ([7bb7548](https://github.com/procoperr/janice/commit/7bb7548ba0b045b6e3f89c525ac35478a0a2884b))

## [0.2.0](https://github.com/procoperr/janice/compare/v0.1.0...v0.2.0) (2025-11-08)


### Features

* add exclude patterns support for directory scanning ([7924712](https://github.com/procoperr/janice/commit/7924712c12bea0c55cfcfaea85395c4a24d140ed))


### Performance Improvements

* switch to Levenshtein, add ahash & rsync stats ([1856df0](https://github.com/procoperr/janice/commit/1856df08a9401b34d747ed4315fc93d90671f819))

## 0.1.0 (2025-11-08)


### Features

* show bytes saved via renames ([c243f37](https://github.com/procoperr/janice/commit/c243f379f09291b19a11942e343464ba1f2ecfff))
