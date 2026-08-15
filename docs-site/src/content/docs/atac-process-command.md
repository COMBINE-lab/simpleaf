---
title: "atac process command"
description: "Map and process single-cell ATAC-seq data, including deterministic barcode correction."
---

`simpleaf atac process` maps a single-cell ATAC-seq library, generates a
corrected cell permit list, and asks alevin-fry to sort and deduplicate the
result into BED records. It can optionally run MACS3 peak calling.

The command requires a piscem ATAC index, barcode reads, an ATAC chemistry, an
output directory, and either paired-end `--reads1`/`--reads2` inputs or
single-end `--reads` input. The supported chemistry names are `10x-v1`,
`10x-v2`, and `10x-multi`.

## Barcode correction

ATAC uses the same deterministic correction controls as RNA:

- `--cell-bc-correction {unique,frequency}` selects how collisions between
  possible corrected targets are handled. The inherited default is `unique`.
- `--cell-bc-neighborhood {hamming-1,substitution-or-shift-1}` selects the
  one-error candidate neighborhood. The inherited ATAC default is `hamming-1`.
- `--cell-bc-confidence <CONFIDENCE>` overrides the Frequency acceptance
  threshold. The inherited ATAC default is `0.90`; decimal and exact-fraction
  forms are accepted.

These options are validated before mapping and then forwarded only to the ATAC
permit-list stage. If they are omitted, simpleaf emits no corresponding flags,
so the protocol defaults remain owned by alevin-fry. The compiled correction
plan produced there is then consumed by ATAC sorting; sorting does not choose a
different correction.

```sh
simpleaf atac process \
  --index /path/to/atac-index \
  --reads1 sample_R1.fastq.gz \
  --reads2 sample_R2.fastq.gz \
  --barcode-reads sample_R3.fastq.gz \
  --chemistry 10x-v2 \
  --cell-bc-correction frequency \
  --cell-bc-confidence 9/10 \
  --output atac_out
```

## Threads

`--threads` is resolved once and passed consistently to mapping, permit-list
generation, and ATAC sorting. Values below two are not rejected: simpleaf logs
a prominent warning, raises the effective value to two, and continues. If the
host reports only one available execution slot, simpleaf warns and still
attempts two threads.

Mapping's gzip decoder shares this thread budget. See [Threads and
decompression](/simpleaf/threads-and-decompression/) for `--decoder` and
`--thread-policy`.

## Outputs

Mapping output is written beneath `af_map/`; corrected and sorted ATAC output is
written beneath `af_process/`. The process log records the exact child commands
and stage timings. `--compress` requests compressed BED output and
`--call-peaks` enables the optional MACS3 stage.
