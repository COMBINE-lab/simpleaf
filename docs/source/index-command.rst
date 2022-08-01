``index`` command
=================

The ``index`` command will take a reference genome FASTA and GTF as input, build a splici reference using the ``pyroe``, and then build a salmon index on the resulting reference. The output directory will contain both a ``ref`` and ``index`` subdirectoy, with the first containing the splici reference that was extracted from the provided genome and GTF, and the latter containing the index built on this reference. The relevant options (which you can obtain by running ``simpleaf index -h``) are:

.. code-block:: console

  simpleaf-index
  build the splici index

  USAGE:
      simpleaf index [OPTIONS] --fasta <FASTA> --gtf <GTF> --rlen <RLEN> --output <OUTPUT>

  OPTIONS:
      -d, --dedup                    deduplicate identical sequences inside the R script when building the splici reference
      -f, --fasta <FASTA>            reference genome
      -g, --gtf <GTF>                reference GTF file
      -h, --help                     Print help information
      -o, --output <OUTPUT>          path to output directory (will be created if it doesn't exist)
      -p, --sparse                   if this flag is passed, build the sparse rather than dense index for mapping
      -r, --rlen <RLEN>              the target read length the index will be built for
      -s, --spliced <SPLICED>        path to FASTA file with extra spliced sequence to add to the index
      -t, --threads <THREADS>        number of threads to use when running [default: 16]
      -u, --unspliced <UNSPLICED>    path to FASTA file with extra unspliced sequence to add to the index
