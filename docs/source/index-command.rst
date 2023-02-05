``index`` command
=================

The ``index`` command has two forms of input; either it will take a reference genome FASTA and GTF as input, build a splici reference using the ``pyroe`` (this is splici-ref mode), or it will take a single reference sequence file (i.e. FASTA file) as input (direct-ref mode).  

In splici-ref mode, after the splici-reference is made with ``pyroe`` the resulting splici reference will be indexed with ``salmon index`` and a copy of the 3-column transcript-to-gene file will be placed in the index directory for subsequent use. The output directory will contain both a ``ref`` and ``index`` subdirectoy, with the first containing the splici reference that was extracted from the provided genome and GTF, and the latter containing the index built on this reference. 

In direct-ref mode, the provided fasta file (passed in with ``--refseq``) will be provided to ``salmon index`` directly.  The output diretory will contain an ``index`` subdirectory that contains the index built on this reference.

The relevant options (which you can obtain by running ``simpleaf index -h``) are:

.. code-block:: console
  
    build the (expanded) reference index
  
    Usage: simpleaf index [OPTIONS] --output <OUTPUT> <--fasta <FASTA>|--ref-seq <REF_SEQ>>
    
    Options:
      -o, --output <OUTPUT>            path to output directory (will be created if it doesn't exist)
      -t, --threads <THREADS>          number of threads to use when running [default: 16]
      -k, --kmer-length <KMER_LENGTH>  the value of k to be used to construct the index [default: 31]
          --keep-duplicates            keep duplicated identical sequences when constructing the index
      -p, --sparse                     if this flag is passed, build the sparse rather than dense index for mapping
      -h, --help                       Print help information
      -V, --version                    Print version information
    
    Expanded Reference Options:
          --ref-type <REF_TYPE>    specify whether an expanded reference, spliced+intronic (or splici) or spliced+unspliced (or spliceu), should be built [default: spliced+intronic]
      -f, --fasta <FASTA>          reference genome to be used for the expanded reference construction
      -g, --gtf <GTF>              reference GTF file to be used for the expanded reference construction
      -r, --rlen <RLEN>            the target read length the splici index will be built for
          --dedup                  deduplicate identical sequences in pyroe when building an expanded reference  reference
          --spliced <SPLICED>      path to FASTA file with extra spliced sequence to add to the index
          --unspliced <UNSPLICED>  path to FASTA file with extra unspliced sequence to add to the index
    
    Direct Reference Options:
          --ref-seq <REF_SEQ>  target sequences (provide target sequences directly; avoid expanded reference construction)
    
    Piscem Index Options:
          --use-piscem                           use piscem instead of salmon for indexing and mapping
      -m, --minimizer-length <MINIMIZER_LENGTH>  the value of m to be used to construct the piscem index (must be < k) [default: 19]
