# Temporary alevin-fry 0.18 integration checklist

This tracked note is the release handoff from alevin-fry 0.18 to simpleaf
0.28. Delete it only after every row is implemented, documented, and tested;
the commit history will retain the context without making this permanent user
documentation.

## Argument forwarding

| Status | simpleaf command | simpleaf option | alevin-fry destination | Inherited default |
|---|---|---|---|---|
| [x] | `quant`, `multiplex-quant` | `--cell-bc-correction` | GPL same name | Unique |
| [x] | `quant`, `multiplex-quant` | `--cell-bc-neighborhood` | GPL same name | Protocol/filter-specific |
| [x] | `quant`, `multiplex-quant` | `--cell-bc-confidence` | GPL same name | 97.5% |
| [x] | `quant`, `multiplex-quant` | `--collate-memory-limit` | collate `--memory-limit` | 2 GiB |
| [x] | `multiplex-quant` | `--sample-bc-correction` | GPL same name | Exact |
| [x] | `multiplex-quant` | `--sample-bc-neighborhood` | GPL same name | Hamming-1 |
| [x] | `multiplex-quant` | `--sample-bc-confidence` | GPL same name | 97.5% |
| [x] | `multiplex-quant` | `--gpl-memory-limit` | GPL `--memory-limit` | 512 MiB |
| [x] | `multiplex-quant` | `--gpl-tmp-dir` | GPL `--tmp-dir` | GPL output directory |
| [x] | `atac process` | cell correction, neighborhood, confidence | ATAC GPL same names | Unique, Hamming-1, 90% |

Unspecified values must be omitted from child commands so alevin-fry remains
the source of truth for resolved protocol defaults. New controls are visible
under advanced barcode-correction or resource help headings.

## Compatibility and validation

- [x] Keep `--sample-correction-mode` accepted but hidden and deprecated.
- [x] Translate legacy `exact` to sample Exact correction.
- [x] Translate legacy `1-edit` to sample Unique plus
      substitution-or-shift-1 without forwarding the deprecated spelling.
- [x] Keep sample-barcode orientation precedence: CLI override, chemistry
      preset, then the alevin-fry default.
- [x] Do not expose `--max-records`, `--collation-mode`, or the `edit-1`
      neighbourhood spelling.
- [x] Validate exact-fraction/decimal confidence and human-readable memory
      values before mapping starts.
- [x] Resolve the pipeline thread count once. Values below two warn, become
      two, and continue; an available-parallelism result of one also warns and
      attempts two.
- [x] Update CLI snapshots and live quant, multiplex, and ATAC documentation.
- [x] Test omitted defaults, every explicit forwarding route, legacy aliases,
      stage-specific memory controls, and thread-floor edge cases.
- [x] Require alevin-fry `>=0.18.0, <1.0.0`.

Validation completed with 71 unit tests, CLI snapshots, CLI smoke/regression
tests, strict rustdoc, strict clippy, formatting, `git diff --check`, and the
Astro production build. Final end-to-end smoke runs remain release gates against
the published alevin-fry 0.18.0 executable.
