``quant`` command
=================


The ``quant`` command takes as input **either**:
  1) the index, reads, and relevant information about the experiment (e.g. the chemistry) OR
  2) the directory containing the result of a previous mapping run, and relevant information about the experiemnt (e.g. the chemistry)

and runs all relevant the steps of the ``alevin-fry`` pipeline. When processing a new dataset from scratch, the first option is the one you are likely interested in (you will provide the ``--index``, ``--reads1`` and ``--reads2`` arguments). **If multiple read files are provided to the** ``--reads1`` **and** ``--reads2`` **arguments, those files must be comma (,) separated.**

On the other hand, if you have already performed quantification or have, for some other reason, already mapped the reads to produce a RAD file, you can start the process from the mapped read directory directly using the ``--map-dir`` argument instead. This latter approach makes it easy to test out different quantification approaches (e.g. different filtering options or UMI resolution strategies). 

**Note**: If you use the unfiltered-permit-list ``-u`` mode for permit-list generation, and you are using either ``10xv2`` or ``10xv3`` chemistry, you can provide the flag by itself, and ``simpleaf`` will automatically fetch and apply the appropriate unifltered permit list.  However, if you are using ``-u`` with any other chemistry, you must explicitly provide a path to the unfiltered permit list to be used.  The ``-d``/``--expected-ori`` flag allows controlling the like-named option that is passed to the ``generate-permit-list`` command of ``alevin-fry``. This is an "optional" option.  If it is not provided explicitly, it is set to "both" (allowing reads aligning in both orientations to pass through), unless the chemistry is set as ``10xv2`` or ``10xv3``, in which case it is set as "fw".  Regardless of the chemistry, if the user sets this option explicitly, this choice is respected.

A note on the ``--chemistry`` flag
----------------------------------

The ``--chemistry`` option can take either a string describing the specific chemisty, or a string describing the geometry of the barcode, umi and mappable read. For example, the string ``10xv2`` and ``10xv3`` will apply the appropriate settings for the 10x chromium v2 and v3 protocols respectively.  However, general geometries can be provided as well, in case the chemistry you are trying to use has not been added as a pre-registered option.  For example, the instead of providing the ``--chemistry`` flag with the string ``10xv2``, you could instead provide it with the string ``"B1[1-16];U1[17-26];R2[1-end]"``, or, instead of providing ``10xv3`` you could provide ``"B1[1-16];U1[17-28];R2[1-end]"``.  The custom geometry flag is passed as a single string and has 3 components, ``B``, ``U`` and ``R`` (all 3 must be present).  Each describes a component of the geometry ``B`` (where the barcode resides), ``U`` (where the UMI resides) and ``R`` (where the biological mappable read resides).  Each piece of this description is separated by a ``;``.  The description itself takes the form of ``{read_index}[{start}-{end}]`` where ``{read_index}`` is either ``1`` or ``2`` and ``{start}-{end}`` describes the 1-indexed range of positions that this part of the geometry occupies.  Finally, there is a special range component ``end`` which is used to signify that this component extends until the end of the observed sequence.  In the examples above ``R2[1-end]`` signifies that the mappable biological sequence consists of *all* of read 2 (i.e. read 2 from position 1 until the end).

.. note::

   If you use a custom geometry frequently, you can add it to a `json` file ``custom_chemistries.json`` in the ``ALEVIN_FRY_HOME`` directory.  This file simply acts as a key-value store mapping each custom geometry to the name you wish to use for it.  For example, putting the contents below into this file would allow you to pass ``--chemistry flarb`` to the ``simpleaf quant`` command, and it would interpret the reads as having the specified geometry (in this case, the same as the ``10xv3`` geometry).  Multiple custom chemistries can be added by simply adding more entries to this `json` file.

   .. code-block:: json
    
      {
        "flarb" : "B1[1-16];U1[17-28];R2[1-end]"
      }

The relevant options (which you can obtain by running ``simpleaf quant -h``) are below:

.. code-block:: bash

  quantify a sample
  
  Usage: simpleaf quant [OPTIONS] --resolution <RESOLUTION> --chemistry <CHEMISTRY> --t2g-map <T2G_MAP> --output <OUTPUT> <--knee|--unfiltered-pl [<UNFILTERED_PL>]|--forced-cells <FORCED_CELLS>|--expect-cells <EXPECT_CELLS>> <--index <INDEX>|--map-dir <MAP_DIR>>

  Options:
    -t, --threads <THREADS>      number of threads to use when running [default: 16]
    -c, --chemistry <CHEMISTRY>  chemistry
    -o, --output <OUTPUT>        output directory
    -h, --help                   Print help information
    -V, --version                Print version information

  mapping options:
    -i, --index <INDEX>            path to index
        --map-dir <MAP_DIR>        path to a mapped output directory containing a RAD file to be quantified
    -1, --reads1 <READS1>          path to read 1 files
    -2, --reads2 <READS2>          path to read 2 files
    -s, --use-selective-alignment  use selective-alignment for mapping (instead of pseudoalignment with structural constraints)

  permit list generation options:
    -d, --expected-ori <EXPECTED_ORI>      The expected direction/orientation of alignments in the chemistry being processed. If not provided, will default to `fw` for 10xv2/10xv3, otherwise `both` [possible values: fw, rc,
                                           both]
    -k, --knee                             use knee filtering mode
    -u, --unfiltered-pl [<UNFILTERED_PL>]  use unfiltered permit list
    -x, --explicit-pl <EXPLICIT_PL>        use a filtered, explicit permit list
    -f, --forced-cells <FORCED_CELLS>      use forced number of cells
    -e, --expect-cells <EXPECT_CELLS>      use expected number of cells
        --min-reads <MIN_READS>            minimum read count threshold for a cell to be retained/processed; only used with --unfiltered-pl [default: 10]

  UMI resolution options:
    -r, --resolution <RESOLUTION>  resolution mode [possible values: cr-like, cr-like-em, parsimony, parsimony-em, parsimony-gene, parsimony-gene-em]
    -m, --t2g-map <T2G_MAP>        transcript to gene map
