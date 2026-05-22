# lcov_filter

`lcov_filter` removes LCOV entries for source lines marked with exclusion
comments.

It is useful when `cargo llvm-cov --lcov` is the coverage source and a project
wants a tiny, dependency-free post-processor for markers such as
`LCOV_EXCL_LINE`.

## Install

```bash
cargo install --git https://github.com/cLazyZombie/lcov_filter
```

## Use

Filter LCOV and keep LCOV output:

```bash
cargo llvm-cov --workspace --lcov --quiet | lcov_filter > lcov.info
```

Print only missing lines:

```bash
cargo llvm-cov --workspace --lcov --quiet | lcov_filter --text
```

Limit text output to paths containing a string:

```bash
cargo llvm-cov --workspace --lcov --quiet | lcov_filter --text --grep my_crate
```

`--text` exits with code `1` when missing lines remain and code `0` when the
filtered result has full line coverage.

## Markers

```rust
let a = 1; // LCOV_EXCL_LINE

// LCOV_EXCL_START
let platform_specific = call_external_tool();
// LCOV_EXCL_STOP
```

Custom marker strings are supported:

```bash
lcov_filter \
  --marker-line COVERAGE_IGNORE_LINE \
  --marker-start COVERAGE_IGNORE_START \
  --marker-stop COVERAGE_IGNORE_STOP
```

## Notes

- LCOV input is read from stdin.
- Source paths are read from `SF:` records.
- Input and source files are processed in memory, so very large reports can use
  noticeable RAM.

## License

MIT
