``flex-quant`` command
======================

The ``flex-quant`` command runs the end-to-end ``simpleaf`` pipeline for 10x Flex Gene Expression data. Unlike :doc:`/quant-command`, which is designed around standard single-cell RNA-seq chemistries and a single cell-barcode whitelist, ``flex-quant`` handles the extra resources and steps required for Flex assays:

- Flex chemistry lookup from the chemistry registry
- probe set selection by organism
- probe-set CSV to FASTA conversion and ``probe_t2g.tsv`` generation
- probe index construction with ``piscem build`` when needed
- cell barcode whitelist resolution
- sample barcode list resolution
- ``piscem map-scrna``
- multi-barcode permit-list generation with ``alevin-fry generate-permit-list``
- ``alevin-fry collate`` and ``alevin-fry quant``

At present, ``flex-quant`` expects a registered Flex chemistry such as ``10x-flexv1-gex-3p`` or ``10x-flexv2-gex-3p`` and requires ``piscem`` plus ``alevin-fry`` to be configured with :doc:`/set-paths`.

Overview
--------

The command needs:

1. a Flex chemistry name via ``--chemistry``
2. an organism via ``--organism`` for automatic probe-set selection
3. paired-end reads via ``--reads1`` and ``--reads2``
4. an output directory via ``--output``

If the chemistry registry contains the needed metadata, ``simpleaf`` can automatically download and cache the probe set, the cell barcode whitelist, and the sample barcode list. If you already have local resources, you can override these defaults with ``--index``, ``--probe-set``, or ``--sample-bc-list``.

The relevant options (which you can obtain by running ``simpleaf flex-quant -h``) are below:

.. code-block:: console

    quantify a 10x Flex GEX sample (probe-based, multiplexed)

    Usage: simpleaf flex-quant [OPTIONS] --chemistry <CHEMISTRY> --organism <ORGANISM> --output <OUTPUT> --reads1 <READS1> --reads2 <READS2>

    Options:
      -c, --chemistry <CHEMISTRY>    Chemistry name: 10x-flexv1-gex-3p or 10x-flexv2-gex-3p
          --organism <ORGANISM>      Target organism for automatic probe set selection [possible values: human, mouse]
      -o, --output <OUTPUT>          Path to output directory
      -t, --threads <THREADS>        Number of threads to use [default: 16]
      -r, --resolution <RESOLUTION>  UMI resolution mode [default: cr-like] [possible values: cr-like, cr-like-em, parsimony, parsimony-em, parsimony-gene, parsimony-gene-em]
      -h, --help                     Print help
      -V, --version                  Print version

    Mapping Options:
      -i, --index <INDEX>    Path to pre-built probe index (overrides auto-build)
      -1, --reads1 <READS1>  Comma-separated list of R1 FASTQ files
      -2, --reads2 <READS2>  Comma-separated list of R2 FASTQ files

    Probe Set Options:
          --probe-set <PROBE_SET>            Path to probe set CSV or FASTA (overrides auto-download)
          --sample-bc-list <SAMPLE_BC_LIST>  Path to sample/probe barcode file with rotation mapping
          --kmer-length <KMER_LENGTH>        k-mer length for probe index building [default: 23]

    Piscem Mapping Options:
          --skipping-strategy <SKIPPING_STRATEGY>  The skipping strategy to use for k-mer collection [default: permissive] [possible values: permissive, strict]
          --struct-constraints                     If piscem >= 0.7.0, enable structural constraints
          --max-ec-card <MAX_EC_CARD>             Maximum cardinality equivalence class to examine [default: 4096]

    Permit List Options:
          --min-reads <MIN_READS>  Minimum read count threshold for unfiltered permit list [default: 10]

Resource resolution
-------------------

``flex-quant`` resolves resources in the following order:

- Probe index:
  If ``--index`` is provided, ``simpleaf`` uses that index directly. The command expects a corresponding ``probe_t2g.tsv`` next to the index, unless you also provide ``--probe-set`` so it can generate the t2g mapping.
- Probe set:
  If ``--probe-set`` is provided, it overrides the registry entry. A CSV probe set is converted into a FASTA plus ``probe_t2g.tsv`` automatically. A FASTA input is accepted as-is, and ``simpleaf`` generates an identity-style t2g mapping from the FASTA headers.
- Automatic probe-set selection:
  If neither ``--index`` nor ``--probe-set`` is provided, ``simpleaf`` looks up the requested ``--organism`` in the selected chemistry's registered probe sets, downloads the matching probe CSV if needed, and builds a cached probe index.
- Cell barcode whitelist:
  This is resolved from the selected chemistry's permit-list metadata in the registry.
- Sample barcode list:
  This is resolved from ``--sample-bc-list`` if provided, otherwise from the selected chemistry's registry metadata.

Examples
--------

Use a registry-backed Flex chemistry with automatic resource resolution:

.. code-block:: console

   $ export ALEVIN_FRY_HOME=/path/to/af_home
   $ simpleaf flex-quant \
       --chemistry 10x-flexv2-gex-3p \
       --organism human \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out

Use local probe-set and sample-barcode files instead of downloading them:

.. code-block:: console

   $ simpleaf flex-quant \
       --chemistry 10x-flexv1-gex-3p \
       --organism mouse \
       --probe-set /path/to/probe_set.csv \
       --sample-bc-list /path/to/sample_bc.tsv \
       --reads1 lane1_R1.fastq.gz,lane2_R1.fastq.gz \
       --reads2 lane1_R2.fastq.gz,lane2_R2.fastq.gz \
       --output flex_out

Use a pre-built probe index:

.. code-block:: console

   $ simpleaf flex-quant \
       --chemistry 10x-flexv2-gex-3p \
       --organism human \
       --index /path/to/probe_index \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out

Output
------

The command creates the requested output directory and writes:

- ``af_map/``: the ``piscem`` mapping output
- ``af_quant/``: the ``alevin-fry`` permit-list, collate, and quantification output
- ``simpleaf_flex_quant_info.json``: a metadata record describing the resolved inputs, executed commands, and step timings

Notes
-----

- ``flex-quant`` is specific to registered Flex GEX chemistries. For standard scRNA-seq chemistries and general custom geometries, use :doc:`/quant-command`.
- The Flex pipeline currently uses ``piscem`` for mapping.
- When a probe CSV is converted, all probes are kept in the generated FASTA and t2g mapping so that downstream quantification has a complete reference-to-gene map.
