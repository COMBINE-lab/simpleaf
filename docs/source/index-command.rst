``index`` command
=================

The ``index`` command has two forms of input; either it will take a reference genome FASTA and GTF as input, from which it can build a spliced+intronic (splici) reference or a spliced+unspliced (spliceu) reference using `roers <https://github.com/COMBINE-lab/roers>`_  (which is used as a library directly from ``simpleaf``, and so need not be installed independently), or it will take a single reference sequence file (i.e. FASTA file) as input (direct-ref mode).  

In expanded reference mode, after the expanded reference is constructed, the resulting reference will be indexed with ``piscem build`` or ``salmon index`` command (depending on the mapper you choose to use), and a copy of the 3-column transcript-to-gene file will be placed in the index directory for subsequent use. The output directory will contain both a ``ref`` and ``index`` subdirectory, with the first containing the splici reference that was extracted from the provided genome and GTF, and the latter containing the index built on this reference. 

In direct-ref mode, if ``--refseq`` is passed, the provided FASTA file will be provided to ``piscem build`` or ``salmon index`` directly.  If ``probe_csv`` or ``feature_csv`` is passed, a FASTA file will be created accordingly and provided to ``piscem build`` or ``salmon index``. The output directory will contain an ``index`` subdirectory that contains the index built on this reference.

- ``probe_csv``: A CSV file containing probe sequences to use for direct reference indexing. The file must follow the format of `10x Probe Set Reference CSV <https://www.10xgenomics.com/support/cytassist-spatial-gene-expression/documentation/steps/probe-sets/visium-ffpe-probe-sets-files#:~:text=probe%20set%20downloads-,Probe%20set%20reference%20CSV%20file,-This%20CSV%20file>`_, containing four mandatory columns: `gene_id`, `probe_seq`, `probe_id`, and `included` (must be ``TRUE`` or ``FALSE``), and an optional column: `region` (must be ``spliced`` or ``unspliced``). When parsing the file, ``simpleaf`` will only use the rows where the `included` column is ``TRUE``. For each row, ``simpleaf`` first builds a FASTA record where the identifier is set as `probe_id`, and the sequence is set as `probe_seq`. Then, it will build a t2g file where the first column is `probe_id` and the second column is `gene_id`. If the `region` column exists, the t2g file will include the region information, so as to trigger the USA mode in ``simpleaf quant`` to generate spliced and unspliced count separately. The t2g file will be identified by ``simpleaf quant`` automatically if ``--t2g-map`` is not set.
- ``feature_csv``: A CSV file containing feature barcode sequences to use for direct reference indexing. The file must follow the format of `10x Feature Reference CSV <https://www.10xgenomics.com/support/software/cell-ranger/latest/analysis/inputs/cr-feature-ref-csv#columns>`_. Currently, only three columns are used: `id`, `name`, and `sequence`. When parsing the file, ``simpleaf`` first builds a FASTA file using the `id` and `sequence` columns. Then, it will build a t2g file where the transcript is set as `id` and the gene is set as `name`. The t2g file will be identified by ``simpleaf quant`` automatically if ``--t2g-map`` is not set.

The relevant options (which you can obtain by running ``simpleaf index -h``) are:

.. code-block:: console

    build the (expanded) reference index

    Usage: simpleaf index [OPTIONS] --output <OUTPUT> <--fasta <FASTA>|--ref-seq <REF_SEQ>|--probe-csv <PROBE_CSV>|--feature-csv <FEATURE_CSV>>

    Options:
    -o, --output <OUTPUT>            Path to output directory (will be created if it doesn't exist)
    -t, --threads <THREADS>          Number of threads to use when running [default: 16]
    -k, --kmer-length <KMER_LENGTH>  The value of k to be used to construct the index [default: 31]
        --gff3-format                Denotes that the input annotation is a GFF3 (instead of GTF)
                                    file
        --keep-duplicates            Keep duplicated identical sequences when constructing the index
        --overwrite                  Overwrite existing files if the output directory is already
                                    populated
    -h, --help                       Print help
    -V, --version                    Print version

    Expanded Reference Options:
        --ref-type <REF_TYPE>    Specify whether an expanded reference, spliced+intronic (or splici)
                                or spliced+unspliced (or spliceu), should be built [default:
                                spliced+intronic]
    -f, --fasta <FASTA>          Path to a reference genome to be used for the expanded reference
                                construction
    -g, --gtf <GTF>              Path to a reference GTF/GFF3 file to be used for the expanded
                                reference construction
    -r, --rlen <RLEN>            The Read length used in roers to add flanking lengths to intronic
                                sequences
        --dedup                  Deduplicate identical sequences in roers when building the expanded
                                reference
        --spliced <SPLICED>      Path to FASTA file with extra spliced sequence to add to the index
        --unspliced <UNSPLICED>  Path to a FASTA file with extra unspliced sequence to add to the
                                index

    Direct Reference Options:
        --feature-csv <FEATURE_CSV>  A CSV file containing feature barcode sequences to use for
                                    direct reference indexing. The file must follow the format of
                                    10x Feature Reference CSV. Currently, only three columns are
                                    used: id, name, and sequence
        --probe-csv <PROBE_CSV>      A CSV file containing probe sequences to use for direct
                                    reference indexing. The file must follow the format of 10x Probe
                                    Set Reference v2 CSV, containing four mandatory columns:
                                    gene_id, probe_seq, probe_id, and included (TRUE or FALSE), and
                                    an optional column: region (spliced or unspliced)
        --ref-seq <REF_SEQ>          A FASTA file containing reference sequences to directly build
                                    index on, and avoid expanded reference construction

    Piscem Index Options:
    -m, --minimizer-length <MINIMIZER_LENGTH>
            Minimizer length to be used to construct the piscem index (must be < k) [default: 19]
        --decoy-paths <DECOY_PATHS>
            Paths to decoy sequence FASTA files used to insert poison k-mer information into the
            index (only if using piscem >= 0.7)
        --seed <HASH_SEED>
            The seed value to use in SSHash index construction (try changing this in the rare event
            index build fails) [default: 1]
        --work-dir <WORK_DIR>
            The working directory where temporary files should be placed [default: ./workdir.noindex]
        --use-piscem
            Use piscem instead of salmon for indexing and mapping (default)

    Alternative salmon-alevin Index Options:
    -p, --sparse     If this flag is passed, build the sparse rather than dense index for mapping
        --no-piscem  Don't use the default piscem mapper, instead, use salmon-alevin


