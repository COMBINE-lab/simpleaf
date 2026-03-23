``multiplex-quant`` command
===========================

The ``multiplex-quant`` command runs the end-to-end ``simpleaf`` pipeline for 10x Flex Gene Expression data and related multi-barcode assays. Unlike :doc:`/quant-command`, which is designed around standard single-cell RNA-seq chemistries and a single cell-barcode whitelist, ``multiplex-quant`` handles the extra resources and steps required for Flex assays:

- Flex chemistry lookup from the chemistry registry
- probe set selection by organism
- probe-set CSV to FASTA conversion and ``probe_t2g.tsv`` generation
- probe index construction with ``piscem build`` when needed
- cell barcode whitelist resolution
- sample barcode list resolution
- ``piscem map-scrna``
- multi-barcode permit-list generation with ``alevin-fry generate-permit-list``
- ``alevin-fry collate`` and ``alevin-fry quant``

At present, ``multiplex-quant`` expects a registered Flex chemistry such as ``10x-flexv1-gex-3p`` or ``10x-flexv2-gex-3p`` and requires ``piscem`` plus ``alevin-fry`` to be configured with :doc:`/set-paths`.

Overview
--------

The command needs:

1. a Flex chemistry name via ``--chemistry``
2. an organism via ``--organism`` for automatic probe-set selection
3. paired-end reads via ``--reads1`` and ``--reads2``
4. an output directory via ``--output``

If the chemistry registry contains the needed metadata, ``simpleaf`` can automatically download and cache the probe set, the cell barcode whitelist, and the sample barcode list. If you already have local resources, you can override these defaults with ``--index``, ``--probe-set``, or ``--sample-bc-list``.

The default output is the standard Matrix Market directory under ``af_quant/alevin``. If you pass ``--anndata-out``, ``simpleaf`` will additionally write an AnnData ``.h5ad`` file at ``af_quant/alevin/quants.h5ad``.

For multiplex output, the resulting AnnData object is intended to preserve the extra sample-level structure of the experiment:

- ``obs_names`` are sample-qualified cell identifiers
- ``obs["cell_barcode"]`` stores the corrected cell barcode without the sample prefix
- ``obs["sample_name"]`` stores the sample / probe-barcode assignment
- ``var["gene_id"]`` remains the matrix feature identifier
- ``var["gene_symbol"]`` is added when a ``gene_id_to_name.tsv`` mapping is available from the probe set or index
- ``uns`` stores the standard ``gpl_info``, ``collate_info``, ``quant_info``, and ``simpleaf_map_info`` records, and for multiplex runs it also stores ``sample_info`` plus ``simpleaf_multiplex_quant_info``

The relevant options (which you can obtain by running ``simpleaf multiplex-quant -h``) are below:

.. code-block:: console

    quantify a multiplexed sample (e.g. 10x Flex, or any custom multi-barcode protocol)

    Usage: simpleaf multiplex-quant [OPTIONS] --output <OUTPUT>

    Options:
      -c, --chemistry <CHEMISTRY>    Chemistry name (e.g. 10x-flexv1-gex-3p). Provides defaults for geometry, cell BC whitelist, sample BC list, and probe set. All can be overridden individually. If omitted, --geometry and --cell-bc-list are required
          --organism <ORGANISM>      Target organism for automatic probe set selection [possible values: human, mouse]
          --cell-bc-list <CELL_BC_LIST>
                                      Path to cell barcode whitelist (one barcode per line, overrides chemistry default)
          --expected-ori <EXPECTED_ORI>
                                      Expected read orientation: fw, rc, or both [default: both]
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

    Reference Options:
      -m, --t2g-map <T2G_MAP>  Path to a transcript-to-gene map file
          --usa                Resolve expression into separate spliced and unspliced counts. This requires splicing-aware probe annotations: either a probe CSV with a ``region`` column containing ``spliced`` / ``unspliced`` values, or a pre-built index with an adjacent 3-column t2g file

    Piscem Mapping Options:
          --skipping-strategy <SKIPPING_STRATEGY>  The skipping strategy to use for k-mer collection [default: permissive] [possible values: permissive, strict]
          --struct-constraints                     If piscem >= 0.7.0, enable structural constraints
          --max-ec-card <MAX_EC_CARD>             Maximum cardinality equivalence class to examine [default: 4096]

    Permit List Options:
          --min-reads <MIN_READS>  Minimum read count threshold for unfiltered permit list [default: 10]

    Output Options:
          --anndata-out  Generate an anndata (h5ad format) count matrix from the standard (matrix-market format) output

Resource resolution
-------------------

``multiplex-quant`` resolves resources in the following order:

- Probe index:
  If ``--index`` is provided, ``simpleaf`` accepts either a ``simpleaf index`` output directory, its ``index/`` subdirectory, the ``piscem_idx`` prefix within that directory, or a multiplex probe-index directory/prefix. It will reuse adjacent metadata and t2g files when present.
- Probe set:
  If ``--probe-set`` is provided, it overrides the registry entry. A CSV probe set is converted into a FASTA plus a gene-level ``probe_t2g.tsv`` automatically, and if probe ``region`` annotations are present it also produces a USA-mode t2g for ``--usa``. A FASTA input is accepted as-is, and ``simpleaf`` generates an identity-style t2g mapping from the FASTA headers.
- Automatic probe-set selection:
  If neither ``--index`` nor ``--probe-set`` is provided, ``simpleaf`` looks up the requested ``--organism`` in the selected chemistry's registered probe sets, downloads the matching probe CSV if needed, and builds a cached probe index.
- Cell barcode whitelist:
  This is resolved from the selected chemistry's permit-list metadata in the registry.
- Sample barcode list:
  This is resolved from ``--sample-bc-list`` if provided, otherwise from the selected chemistry's registry metadata.

USA-mode requirements
---------------------

``--usa`` is optional. If it is not provided, ``multiplex-quant`` collapses probe expression to the gene level even when splicing annotations are available.

If ``--usa`` is provided, the reference must carry splicing-aware annotations:

- For probe CSV input, the CSV must contain a ``region`` column and each included probe must have value ``spliced`` or ``unspliced``.
- For pre-built indices, ``simpleaf`` must be able to find an adjacent 3-column t2g such as ``t2g_3col.tsv`` or ``probe_t2g_usa.tsv``.
- FASTA probe sets do not encode splicing status, so they are not compatible with ``--usa`` unless you also provide an explicit splicing-aware ``--t2g-map``.

If the required splicing annotations are not available, ``simpleaf`` will stop with an error that explains which input is missing the needed information and suggests rerunning without ``--usa``.

Examples
--------

Use a registry-backed Flex chemistry with automatic resource resolution:

.. code-block:: console

   $ export ALEVIN_FRY_HOME=/path/to/af_home
   $ simpleaf multiplex-quant \
       --chemistry 10x-flexv2-gex-3p \
       --organism human \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out

Use local probe-set and sample-barcode files instead of downloading them:

.. code-block:: console

   $ simpleaf multiplex-quant \
       --chemistry 10x-flexv1-gex-3p \
       --organism mouse \
       --probe-set /path/to/probe_set.csv \
       --sample-bc-list /path/to/sample_bc.tsv \
       --reads1 lane1_R1.fastq.gz,lane2_R1.fastq.gz \
       --reads2 lane1_R2.fastq.gz,lane2_R2.fastq.gz \
       --output flex_out

Use a pre-built probe index:

.. code-block:: console

   $ simpleaf multiplex-quant \
       --chemistry 10x-flexv2-gex-3p \
       --organism human \
       --index /path/to/simpleaf_index_output \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out

Request AnnData output in addition to the Matrix Market output:

.. code-block:: console

   $ simpleaf multiplex-quant \
       --chemistry 10x-flexv1-gex-3p \
       --organism human \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out \
       --anndata-out

Request USA-mode probe quantification:

.. code-block:: console

   $ simpleaf multiplex-quant \
       --chemistry 10x-flexv2-gex-3p \
       --organism human \
       --probe-set /path/to/probe_set.csv \
       --usa \
       --reads1 sample_R1.fastq.gz \
       --reads2 sample_R2.fastq.gz \
       --output flex_out

Output
------

The command creates the requested output directory and writes:

- ``af_map/``: the ``piscem`` mapping output
- ``af_quant/``: the ``alevin-fry`` permit-list, collate, and quantification output
- ``af_quant/simpleaf_map_info.json``: parsed mapping metadata copied into the quantification directory for downstream consumers such as AnnData conversion
- ``af_quant/simpleaf_multiplex_quant_info.json``: multiplex pipeline metadata copied into the quantification directory so it can be embedded into AnnData ``uns``
- ``af_quant/gene_id_to_name.tsv``: optional gene ID to gene symbol/name mapping copied when available from the probe set or index
- ``af_quant/alevin/quants.h5ad``: optional AnnData output written when ``--anndata-out`` is requested
- ``simpleaf_multiplex_quant_info.json``: a metadata record describing the resolved inputs, executed commands, and step timings

Notes
-----

- ``multiplex-quant`` is specific to registered Flex GEX chemistries and related multi-barcode protocols. For standard scRNA-seq chemistries and general custom geometries, use :doc:`/quant-command`.
- The Flex pipeline currently uses ``piscem`` for mapping.
- By default, probe expression is grouped at the gene level. Pass ``--usa`` only when the input probe set or pre-built index carries explicit splicing annotations.
