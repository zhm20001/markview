# Security verification

This page tracks security evidence, proposed harnesses, and outstanding work. [Security and threat model](security.md) owns the policy; [Security reference](security-reference.md) owns threat IDs and implementation details. The invariant IDs below are retained for traceability: some are historical proposals rather than current policy or established guarantees.

## Security invariants

| | Invariant | Status |
| --- | --- | --- |
| I1 | No input at or below the accepted size aborts, panics, hangs, or exhausts memory | T1 fixed with a regression test; T2 bounded; T3 still unmeasured |
| I2 | Every recursion has an explicit depth bound whose violation is a recoverable error | Covered for inline and block parsing by `Limits::inline_depth` and `Limits::block_depth`, both degrading to text; not a proof about every dependency |
| I3 | Decoded pixel totals, cache residency, and concurrent decodes are bounded per document | Partially: per-image and cache bounds exist, totals do not |
| I4 | Document content produces no effect outside the process unless the user confirms an action whose target the document cannot forge | Historical proposal, not current policy: relative image reads, default remote requests, and allowlisted clicked links need no confirmation |
| I5 | Every filesystem path is confined to the document directory subtree | **Deliberately not enforced.** Local image sources are relative-only; links are not; see [T5](security-reference.md#t5-arbitrary-local-file-read) |
| I6 | Rendering the same input is deterministic across runs and processes | Satisfied for image selection: `Prepared::images` is a `BTreeMap`, so the sole-image caption path is deterministic |
| I7 | Every output geometry value is finite and bounded in magnitude | Unverified; the stylesheet validates only finiteness and sign, so a finite but absurd size such as `1e30` passes |
| I8 | The only URL Markview opens comes from a single scheme allowlist, with no second path | Satisfied: `src/link.rs` is the only policy, and `openable_link` no longer exists as a second one |
| I9 | The number of remote requests and total bytes triggered by one document is bounded | Default admission cap: 128 distinct sources per revision, lifted by Load all; 32 MiB per body and 128 MiB disk cache. These are not a universal bound on request count or lifetime traffic |
| I10 | Any path handed to the OS has had its executability judged for that platform and confirmed by the user | Historical proposal, not current policy: directories and allowlisted extensions bypass confirmation, and executable permission bits are not checked |

## Mitigations

Implemented structural controls include the shared core `Limits`, inline-recursion depth limits, budgeted greedy line breaking, and centralized link classification. Directory containment was deliberately rejected; see the [image-path policy](security.md#image-paths).

Tests use committed font subsets with system fonts hidden by `tests/fontconfig.conf`; see [Development](development.md). This supersedes the older blanket TODO to pin test fonts, but does not establish cross-process layout determinism (I6).

Engineering, in order of cost:

| Step | Cost | Covers | Status |
| --- | --- | --- | --- |
| Run the test suite under `cargo careful` | Minutes | Undefined behavior in dependencies that the standard library can detect | Open |
| Dependency audit tooling (`cargo-deny`, `cargo-audit`, `cargo-vet`) | Minutes | T4 supply chain | Open, and previously misreported as done |
| ASAN-instrumented fuzzing of shaping, highlighting, math, and PDF export | Hours | T4 | Open. Decode and SVG rasterization wait until that code can be linked; see the [deferral note](#verification-plan). |
| `loom` or `shuttle` models of `Worker` and `Images` | Days | T9 | Open |
| `kani` proofs for the integer and slice logic in `html`, image-source resolution, and the link allowlist | Days | Boundary errors in I8 and the T6 classes. Not applicable to the float-heavy line breaker, where Kani's support is poor | Open |
| `miri` over the dependency-free logic modules | Days | Requires extracting those modules, since the font stack is FFI | Open |

## Dependency coverage

The earlier investigation recorded the following coverage assessment; upstream coverage has not been rechecked for this reorganization: the `image` crate is already continuously fuzzed upstream in OSS-Fuzz, so byte-level raster fuzzing would largely repeat that work. The parts that are *not* covered upstream are Markview's own decode paths: the hand-written ICO entry scan in `src/images/decode.rs`, first-frame selection for APNG and animated WebP, the SVG path that rasterizes at a caller-supplied target size, and the premultiplied-to-straight alpha conversion. No OSS-Fuzz project for `resvg` or `usvg` was found, so SVG rendering is the least covered layer in the stack. No dependency-audit tooling is configured in the repository yet.

## Verification plan

Each target needs a harness with an oracle, rather than only a no-crash check. Prioritize deeper pipeline stages, structured inputs, and properties such as bounded work and consistent output. Proposed harnesses below are not evidence that those properties already hold.

As of 2026-10-02 the fuzzing plan that superseded the harness work in this section is implemented as the [`fuzz/`](../fuzz/) crate: fifteen libFuzzer targets over the library crates, structure-aware mutators, calibrated per-input wall and allocation budgets, and differential oracles; library bugs it finds are fixed and regression-tested in the crate they belong to. This page stays the catalogue of targets, threats, and invariants; `fuzz/README.md` owns the target list, the oracles, the budgets and the corpora, and says what each target looks for and how. Two boundaries from that plan bound what this page may claim:

- Only the library crates are reachable from a harness. The input handling in the binary crate — image decoding, SVG rasterization, Mermaid rendering, and image-source resolution — is **deferred until that code is extracted and can be linked**. No harness for those modules exists yet.
- Link policy, stylesheet discovery and installation, settings storage, network and font downloads, and CLI parsing are outside fuzzing scope; their existing unit tests remain the only evidence.

| Target | Harness | Oracle | Status |
| --- | --- | --- | --- |
| I1, I2 | Parse and layout, plus a dedicated line-breaker harness over unit vectors | No abort, no panic, a recorded recursion bound, a time budget | Regression tests exist for the 12 KB abort and for greedy termination; parse and layout are fuzzed under wall and allocation budgets (`fuzz/fuzz_targets/parse`, `layout`) |
| I3, I9 | Layout with a synthetic image snapshot; decode in isolation | A counting global allocator asserting peak allocation against input length; a request counter | The counting global allocator exists in `fuzz/fuzzlib` and gates every target; request counting is unit-tested. Decode in isolation is deferred with the other binary-crate modules |
| Link policy (replaces I4), I8 | Source resolution over `(link, document path)` | The T6 class tables | Implemented as `src/link.rs` tests |
| Image-path policy (replaces I5) | Source resolution over `(src, document path)` | The relative-only predicate | Implemented as `src/images/tests.rs` tests |
| I6 | Layout twice in one process and across processes | Field-by-field equality after serialization | Progressive-versus-final equality is fuzzed (`fuzz/fuzz_targets/layout_diff`); cross-process equality is open |
| I7 | Layout over generated numeric options | Finiteness and magnitude bounds on every geometry value | Fuzzed: `geometry` feeds `f32` bit patterns to the render primitives and layout asserts snapshot finiteness |
| T4 | Shaping, highlighting, math, and PDF export | ASAN cleanliness | Fuzzed under ASAN (`shaping`, `highlight`, `math`, `pdf`); decode and SVG rasterization deferred |
| T2 | Highlight, math, and line breaking, each with a timeout | Wall-clock budget per input | Wall-clock budgets are enforced per input by `fuzzlib::budget`; the budget defaults are calibrated, not guessed |
| T7 | Address policy and cap | An address class table and a per-revision counter | Implemented as `src/net.rs` address-policy tests and `src/images/tests.rs` cap tests |
| Differential | Cached versus uncached layout; progressive prefix versus final snapshot | Field-by-field equality. Both properties are already asserted in unit tests and should be enforced during fuzzing | Incremental-reparse equality and progressive-prefix equality are fuzzed; cached-versus-uncached is unit-tested |

Input generation is structured rather than byte-level: the text targets mutate whole lines and Markdown markers (`fuzz/fuzzlib/src/mutators.rs`), and the geometry target derives its input through `arbitrary`. Comrak exposes an `arbitrary` feature that derives `Arbitrary` for its option types, which makes randomized configuration free, and the comrak repository already carries a fuzz suite whose targets include a complexity-focused one and a `sourcepos`-focused one that exercises the same positions `Reader::range` depends on. See [Existing fuzzing assets](#existing-fuzzing-assets); upstream capabilities must be checked before adapting them.

## Existing fuzzing assets

These links are retained from the original investigation as candidate resources. Target counts, version compatibility, and upstream coverage are historical notes to verify before reuse, not claims of current coverage.

| Asset | Use |
| --- | --- |
| [comrak's own fuzz suite](https://github.com/kivikakk/comrak/blob/master/fuzz/Cargo.toml) | Nine targets covering parse, CommonMark, GFM, source positions, footnotes, all-options, CLI defaults, and a complexity-focused target. Markview depends on the same version, so it adapts with a path change. |
| comrak's `arbitrary` feature | Derives `Arbitrary` for the option types, so randomized configuration needs no generator. |
| [pulldown-cmark's `commonmark_js` target](https://github.com/pulldown-cmark/pulldown-cmark/blob/master/fuzz/fuzz_targets/commonmark_js.rs) | A template for differential fuzzing: it normalizes event streams and compares them against commonmark.js running under mozjs. An earlier note here cited a `pandoc` target; no such target exists upstream. |
| [tree-crasher](https://github.com/langston-barrett/tree-crasher) with [tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown) | Grammar-aware mutation without instrumentation. tree-crasher publishes front ends for C, CSS, HTML, JavaScript, Nix, OpenSCAD, Python, Regex, Ruby, Rust, Solidity, SQL, and TypeScript — the list has grown, and still has no Markdown. The grammar exists, so adding a `tree-crasher-markdown` crate is small. Its HTML front end applies to the raw HTML subset directly. |
| [codec-corpus](https://docs.rs/codec-corpus) | Image test corpora, including PngSuite. It is not a dependency of this workspace, and the crate ships no data: datasets are fetched lazily on first access and cached locally. |
| [OSS-Fuzz: cmark](https://github.com/google/oss-fuzz/tree/master/projects/cmark) and [md4c](https://github.com/google/oss-fuzz/tree/master/projects/md4c) | Reference engineering for Markdown fuzzing, including corpus and dictionary layout. |
| [OSS-Fuzz: image-rs](https://github.com/google/oss-fuzz/tree/master/projects/image-rs) | Confirms the `image` crate is fuzzed upstream; do not duplicate it. |
| [librsvg's OSS-Fuzz work](https://gitlab.gnome.org/GNOME/librsvg/-/work_items/1096) | SVG seed corpus and a render-focused target. No equivalent project was found for `resvg` or `usvg`. |
| CommonMark and GFM spec suites, KaTeX test cases | Seeds for parsing and math. Correctness corpora, not crash corpora; pair them with generated extremes. |
| [bolero](https://github.com/camshaft/bolero) | One harness that runs as a coverage-guided fuzzer, a property test, or a Kani proof. |
| [aretext's report](https://devnonsense.com/posts/aretext-markdown-fuzz-test/) | Historical report of 30 billion inputs over 15 days, with 594 interesting inputs and no bugs; motivation to use stronger oracles, not evidence about Markview. |
