---
title: "Threads and decompression"
description: "How simpleaf's -t budget is shared between mapping and gzip decompression, and how to steer that split."
---

Starting with `piscem` 0.22.0, the `-t`/`--threads` value you give `simpleaf` is a
**single budget of execution slots shared between read mapping and gzip
decompression**, rather than a mapping-thread count with decompression happening
somewhere off the books. A broker inside `piscem` decides how to split that budget
and re-solves the split while the run proceeds.

This page explains the two options that steer it. Both are forwarded verbatim to
`piscem`, and both are available on [quant](/simpleaf/quant-command/),
[multiplex-quant](/simpleaf/flex-quant-command/), and `simpleaf atac process`.

:::note
You do not need to set either of these. The default (`--decoder auto`) measures the
input and adapts. They exist for the cases where the automatic choice is wrong, or
where you want a run to be reproducible in its resource use rather than adaptive.
:::

## Why the budget is shared

Gzip decompression is not free, and for a fast mapper against a small index it can
be the limiting stage. Decompressing a `.fastq.gz` serially caps the whole pipeline
at the speed of one decompressor, no matter how many threads the mapper has. The
parallel decoder removes that cap, but only by *spending threads* — threads the
mapper would otherwise use.

Treating `-t` as one budget makes that trade explicit and bounded: a run given
`-t 16` uses 16 slots in total, and the broker moves slots between mapping and
decoding as the observed bottleneck shifts.

## `--decoder`

```sh
simpleaf quant -t 32 --decoder auto ...
```

| Value | Behaviour |
| --- | --- |
| `auto` *(default)* | Let `piscem` adapt the mapping/decode split during the run. |
| `serial` | One decompressor per input; mapping gets the rest of the budget. |
| `parallel` | Force the parallel decoder wherever the input permits it. |
| `parallel=N` | Fix exactly `N` decode slots per gzip input and stop adapting. |

Two things are worth knowing about `parallel`:

- **Not every input can be decoded in parallel.** The parallel decoder needs to seek
  within the compressed stream. Inputs that cannot be read positionally — FIFOs,
  named pipes, process substitution (`<(zcat ...)`) — stay serial no matter what you
  ask for. This is a property of the input, not a failure, and it is reported in
  `map_info.json`.
- **`parallel` is not automatically faster.** On a run where mapping is already the
  bottleneck, moving slots to the decoder makes the run slower. `auto` exists
  because the right answer depends on the index, the chemistry, the compression
  level of the input, and how many threads you gave it.

A useful case for `parallel=N` is a shared machine, where you want the resource
profile of the run to be predictable rather than responsive to load.

## `--thread-policy`

```sh
simpleaf quant -t 32 --thread-policy policy.json ...
```

A JSON file overriding the measured defaults. Every field is optional; an
unrecognised field is an **error**, not a silently ignored no-op, so a typo fails the
run instead of quietly doing nothing.

Currently understood:

```json
{
  "parallel_decode": {
    "min_threads_per_stream": 8
  }
}
```

`min_threads_per_stream` is the number of slots that must be free per gzip input
before the parallel decoder is engaged at all. Lower it to make `piscem` willing to
parallelise decoding on smaller budgets; raise it to keep small runs fully serial.

## Checking what actually happened

The mapping directory's `map_info.json` records the thread budget, the split that
was chosen, and whether the parallel decoder was engaged for each input. If you are
tuning these options, that file — not wall-clock alone — is the thing to read.
