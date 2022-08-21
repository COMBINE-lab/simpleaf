``index`` command
=================

The ``index`` command has two forms of input; either it will take a reference genome FASTA and GTF as input, build a splici reference using the ``pyroe`` (this is splici-ref mode), or it will take a single reference sequence file (i.e. FASTA file) as input (direct-ref mode).  

In splici-ref mode, after the splici-reference is made with ``pyroe`` the resulting splici reference will be indexed with ``salmon index`` and a copy of the 3-column transcript-to-gene file will be placed in the index directory for subsequent use. The output directory will contain both a ``ref`` and ``index`` subdirectoy, with the first containing the splici reference that was extracted from the provided genome and GTF, and the latter containing the index built on this reference. 

In direct-ref mode, the provided fasta file (passed in with ``--refseq``) will be provided to ``salmon index`` directly.  The output diretory will contain an ``index`` subdirectory that contains the index built on this reference.

The relevant options (which you can obtain by running ``simpleaf index -h``) are:

.. code-block:: console

   USAGE:
    simpleaf index [OPTIONS] --output <OUTPUT> <--fasta <FASTA>|--refseq <REFSEQ>>

    OPTIONS:
        -o, --output <OUTPUT>              path to output directory (will be created if it doesn't exist)
        -h, --help                         Print help information
        -k, --kmer-length <KMER_LENGTH>    the value of k that should be used to construct the index [default: 31]
        -p, --sparse                       if this flag is passed, build the sparse rather than dense index for mapping
        -t, --threads <THREADS>            number of threads to use when running [default: 16]

    splici-ref:
        -f, --fasta <FASTA>            reference genome to be used for splici construction
        -g, --gtf <GTF>                reference GTF file
        -r, --rlen <RLEN>              the target read length the index will be built for
        -s, --spliced <SPLICED>        path to FASTA file with extra spliced sequence to add to the index
        -u, --unspliced <UNSPLICED>    path to FASTA file with extra unspliced sequence to add to the index
        -d, --dedup                    deduplicate identical sequences inside the R script when building the splici reference

    direct-ref:
            --refseq <REFSEQ>    target sequences (provide target sequences directly; avoid splici construction)

