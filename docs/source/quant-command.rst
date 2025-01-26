``quant`` command
=================


The ``quant`` command takes as input **either**:
  1) the index, reads, and relevant information about the experiment (e.g. the chemistry) OR
  2) the directory containing the result of a previous mapping run, and relevant information about the experiemnt (e.g. the chemistry)

and runs all relevant the steps of the ``alevin-fry`` pipeline. When processing a new dataset from scratch, the first option is the one you are likely interested in (you will provide the ``--index``, ``--reads1`` and ``--reads2`` arguments). **If multiple read files are provided to the** ``--reads1`` **and** ``--reads2`` **arguments, those files must be comma (,) separated.**

On the other hand, if you have already performed quantification or have, for some other reason, already mapped the reads to produce a RAD file, you can start the process from the mapped read directory directly using the ``--map-dir`` argument instead. This latter approach makes it easy to test out different quantification approaches (e.g. different filtering options or UMI resolution strategies). 

**Note**: If you use the unfiltered-permit-list ``-u`` mode for permit-list generation, and you are using either ``10xv2`` or ``10xv3`` chemistry, you can provide the flag by itself, and ``simpleaf`` will automatically fetch and apply the appropriate unifltered permit list.  However, if you are using ``-u`` with any other chemistry, you must explicitly provide a path to the unfiltered permit list to be used.  The ``-d``/``--expected-ori`` flag allows controlling the like-named option that is passed to the ``generate-permit-list`` command of ``alevin-fry``. This is an "optional" option.  If it is not provided explicitly, it is set to "both" (allowing reads aligning in both orientations to pass through), unless the chemistry is set as ``10xv2`` or ``10xv3``, in which case it is set as "fw".  Regardless of the chemistry, if the user sets this option explicitly, this choice is respected.

The default output format is a Matrix Market format sparse matrix with the relevant counts.  However, if you pass the ``--anndata-out`` flag to the ``quant`` command (in addition to the normal ``-o`` argument to specify the output directory), then additionally an `AnnData <https://anndata.readthedocs.io/en/stable/>`__ file will be created, which should be directly usable in downstream workflows expecting this data type.

A note on the ``--chemistry`` flag
----------------------------------

.. note::

  The geometry specification language has changed in ``simpleaf`` v0.9.0 and above. This change is to unify the geometry description language between ``simpleaf`` and the tools in the backend that actually perform the fragment mapping.  Further, the new laguage is more general, capable and exensible, so it will be easier to add more features in the future in a backward compatible manner.  However, this means that if you have a ``custom_chemistries.json`` file from before ``simpleaf`` v0.9.0, you will have to re-create that file with the new chemistries by overwriting them with the custom geometry descriptions in the new format.

The ``--chemistry`` option can take either a string describing the specific chemisty, or a string describing the geometry of the barcode, umi and mappable read. For example, the string ``10xv2`` and ``10xv3`` will apply the appropriate settings for the 10x chromium v2 and v3 protocols respectively.  However, general geometries can be provided as well, in case the chemistry you are trying to use has not been added as a pre-registered option.  For example, the instead of providing the ``--chemistry`` flag with the string ``10xv2``, you could instead provide it with the string ``"1{b[16]u[10]x:}2{r:}"``, or, instead of providing ``10xv3`` you could provide ``"1{b[16]u[12]x:}2{r:}"``.  

The custom format is as follows; you must specify the content of read 1 and read 2 in terms of the barcode, UMI, and mappable read sequence. A specification looks like this:

.. code-block:: console
  
  1{b[16]u[12]x:}2{r:}


In particular, this is how one would specify the 10x Chromium v3 geometry using the custom syntax.  The format string says that the read pair should be interpreted as read 1 ``1{...}`` followed by read 2 ``2{...}``.  The syntax inside the ``{}`` says how the read should be interpreted.  Here ``b[16]u[12]x:`` means that the first 16 bases constitute the barcode, the next 12 constitute the UMI, and anything that comes after that (if it exists) until the end of read 1 should be discarded (``x``).  For read 2, we have ``2{r:}``, meaning that we should interpret read 2, in it's full length, as biological sequence.

It is possible to have pieces of geometry repeated, in which case they will be extracted and concatenated together.  For example, ``1{b[16]u[12]b[4]x:}`` would mean that we should obtain the barcode by extracting bases 1-16 (1-based indexing) and 29-32 and concatenating them together to obtain the full barcode.  A

.. note::

   If you use a custom geometry frequently, you can add it to the chemistries registry. For details on adding your own chemistry definition to the registry, please read about the :ref:`simpleaf chemistry<chemistry-command>` command.

The relevant options (which you can obtain by running ``simpleaf quant -h``) are below:


.. code-block:: console

    quantify a sample

    Usage: simpleaf quant [OPTIONS] --chemistry <CHEMISTRY> --output <OUTPUT> --resolution <RESOLUTION> <--expect-cells <EXPECT_CELLS>|--explicit-pl <EXPLICIT_PL>|--
    forced-cells <FORCED_CELLS>|--knee|--unfiltered-pl [<UNFILTERED_PL>]> <--index <INDEX>|--map-dir <MAP_DIR>>

    Options:
      -c, --chemistry <CHEMISTRY>  The name of a registered chemistry or a quoted string representing a custom geometry specification
      -o, --output <OUTPUT>        Path to the output directory
      -t, --threads <THREADS>      Number of threads to use when running [default: 16]
      -h, --help                   Print help
      -V, --version                Print version

    Mapping Options:
      -i, --index <INDEX>            Path to a folder containing the index files
      -1, --reads1 <READS1>          Comma-separated list of paths to read 1 files. The order must match the read 2 files
      -2, --reads2 <READS2>          Comma-separated list of paths to read 2 files. The order must match the read 1 files
          --no-piscem                Don't use the default piscem mapper, instead, use salmon-alevin
          --use-piscem               Use piscem for mapping (requires that index points to the piscem index)
      -s, --use-selective-alignment  Use selective-alignment for mapping (only if using salmon alevin as the underlying mapper)
          --map-dir <MAP_DIR>        Path to a mapped output directory containing a RAD file to skip mapping

    Piscem Mapping Options:
          --struct-constraints                         If piscem >= 0.7.0, enable structural constraints
          --ignore-ambig-hits                          Skip checking of the equivalence classes of k-mers that were too ambiguous to be otherwise considered (passing
                                                       this flag can speed up mapping slightly, but may reduce specificity)
          --no-poison                                  Do not consider poison k-mers, even if the underlying index contains them. In this case, the mapping results
                                                       will be identical to those obtained as if no poison table was added to the index
          --skipping-strategy <SKIPPING_STRATEGY>      The skipping strategy to use for k-mer collection [default: permissive] [possible values: permissive, strict]
          --max-ec-card <MAX_EC_CARD>                  Determines the maximum cardinality equivalence class (number of (txp, orientation status) pairs) to examine
                                                       (cannot be used with --ignore-ambig-hits) [default: 4096]
          --max-hit-occ <MAX_HIT_OCC>                  In the first pass, consider only collected and matched k-mers of a read having <= --max-hit-occ hits [default:
                                                       256]
          --max-hit-occ-recover <MAX_HIT_OCC_RECOVER>  If all collected and matched k-mers of a read have > --max-hit-occ hits, then make a second pass and consider
                                                       k-mers having <= --max-hit-occ-recover hits [default: 1024]
          --max-read-occ <MAX_READ_OCC>                Threshold for discarding reads with too many mappings [default: 2500]

    Permit List Generation Options:
      -k, --knee                             Use knee filtering mode
      -u, --unfiltered-pl [<UNFILTERED_PL>]  Use unfiltered permit list
      -f, --forced-cells <FORCED_CELLS>      Use forced number of cells
      -x, --explicit-pl <EXPLICIT_PL>        Use a filtered, explicit permit list
      -e, --expect-cells <EXPECT_CELLS>      Use expected number of cells
      -d, --expected-ori <EXPECTED_ORI>      The expected direction/orientation of alignments in the chemistry being processed. If not provided, will default to `fw`
                                             for 10xv2/10xv3, otherwise `both` [possible values: fw, rc, both]
          --min-reads <MIN_READS>            Minimum read count threshold for a cell to be retained/processed; only use with --unfiltered-pl [default: 10]

    UMI Resolution Options:
      -m, --t2g-map <T2G_MAP>        Path to a transcript to gene map file
      -r, --resolution <RESOLUTION>  UMI resolution mode [possible values: cr-like, cr-like-em, parsimony, parsimony-em, parsimony-gene, parsimony-gene-em]

    Output Options:
          --anndata-out  Generate an anndata (h5ad format) count matrix from the standard (matrix-market format) output

