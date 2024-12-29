``chemistry`` command
=====================

The ``chemistry`` command allows operation (e.g. adding or removing) on custom chemistries to ``simpleaf``'s registry of recognized custom chemistries, and also allows 
inspecting the information associated with a specific chemistry. The command currently has 4 sub-commands: ``add``, ``remove``, ``refresh``, and ``lookup``.  

.. code-block:: bash
    operate on or inspect the chemistry registry

    Usage: simpleaf chemistry <COMMAND>

    Commands:
      refresh  Add or refresh chemistry definitions from the upstream repository
      add      Add a new chemistry to the registry of custom chemistries
      remove   Remove a chemistry from the chemistry registry
      lookup   Lookup a chemistry in the chemistry registry
      help     Print this message or the help of the given subcommand(s)

    Options:
      -h, --help     Print help
      -V, --version  Print version

These sub-commands are described below.

``simpleaf chemistry refresh``
-----------------------

The ``refresh`` sub-command takes no arguments, it consults the remote ``simpleaf`` GitHub repository to check for an updated chemistry registry, and, adds any new chemistries from that registry or updates the entries for any chemistries in that registry whose version number has increased.

``simpleaf chemistry add``
-------------------

The ``add`` sub-command has the usage shown below:

.. code-block:: bash
    Add a new chemistry to the registry of custom chemistries

    Usage: simpleaf chemistry add [OPTIONS] --name <NAME> --geometry <GEOMETRY> --expected-ori <EXPECTED_ORI>

    Options:
      -n, --name <NAME>                  the name to give the chemistry
      -g, --geometry <GEOMETRY>          the geometry to which the chemistry maps, wrapped in quotes
      -e, --expected-ori <EXPECTED_ORI>  the expected orientation to give to the chemistry [possible values: fw, rc, both]
          --local-url <LOCAL_URL>        the (fully-qualified) path to a local file that will be copied into the permit list directory of the ALEVIN_FRY_HOME directory to provide a permit list for use with this chemistry
          --remote-url <REMOTE_URL>      the url of a remote file that will be downloaded (*on demand*) to provide a permit list for use with this chemistry. This file should be obtainable with the equivalent of `wget <local-url>`. The file will only be downloaded the
                                         first time it is needed and will be locally cached in ALEVIN_FRY_HOME after that
          --version <VERSION>            optionally assign a version number to this chemistry. A chemistry's entry can be updated in the future by adding it again with a higher version number
      -h, --help                         Print help


This command allows the user to register a new chemistry or modifying an existing chemistry (by providing the ``--overwrite`` parameter).  Once a chemistry is registered, ``simpleaf`` will be able to lookup certain information about this chemistry when other commands are invoked, so as to avoid passing potentially long command-line flags in future invocations repeatedly for this chemistry.

Every chemistry added to the registry has 3 mandatory associated properties: a ``name``, a ``geometry`` specification, and an ``expected-ori``. 

- ``name``: The name of the chemistry must be a unique (within the existing registry) UTF-8 identifier. If the name has been registered, the existing definition will be overwritten if specifying``--overwrite``. Otherwise, simpleaf will complain and fail.
- ``geometry``: The geometry specification should be provided enclosed in quotes, and follow `Sequence Fragment Geometry Description Language <https://hackmd.io/@PI7Og0l1ReeBZu_pjQGUQQ/rJMgmvr13>`_ as would be provided to the `quant command <https://simpleaf.readthedocs.io/en/latest/quant-command.html#a-note-on-the-chemistry-flag>`. 
- ``expected-ori``: The expected orientation of a chemistry must be one of ``fw``, ``rc``, or ``both``. It represents the expected orientation with respect to the first (most upstream) mappable biological sequence. Imagine we have reads from 10x Chromium 5' protocols where read1s and reads2s are both of length 150 base pairs. In this case, after passing the cell barcode, UMI, and a fixed fragment, the rest of read1s will be in the forward orientation and the read2s will be in the reverse complementary orientation. If we map the biological sequence in read1s and read2s as paired-end reads (currently only supported when using the default mapper -- piscem), as biological read1s are the first mappable sequences, the expected orientation for this chemistry should be ``fw``. However, if we only map read2s, the expected orientation should be ``rc``, because read2s are the first mappable sequences and are in the reverse complementary orientation.

In addition to the required fields, there are 3 optional fields: 

- ``local-url``: A fully-qualified path to a file containing the permit list (i.e. whitelist) of cell barcodes.
- ``remote-url``:  A remote URL providing a location from which a permit list can be downloaded .
- ``version``: A `semver <https://semver.org/>`_ format version tag indicating the version of the chemistry definition. It is NOT the version or revision of the physical chemistry itself, e.g., as the V2 or V3 in chromium V2 or chromium V3.

**Note** any file provided via the ``local-url`` will be *copied* into a subdirectory of the ``ALEVIN_FRY_HOME`` directory. To avoid this copying, you can procide the file directly to the simpleaf commands that take the file, for example, ``simpleaf quant -u /path/to/your/large/file``.

``simpleaf chemistry remove``
----------------------

The ``remove`` sub-command has the usage shown below:

.. code-block:: bash
    Remove a chemistry from the chemistry registry

    Usage: simpleaf chemistry remove --name <NAME>

    Options:
      -n, --name <NAME>  the name of the chemistry you wish to remove
      -h, --help         Print help
      -V, --version      Print version

The single required argument ``--name`` should be the key (name) of an existing chemistry in the current registry. If the key (name) of any chemistry matches, it will be removed from the registry.

``simpleaf chemistry lookup``
----------------------

The ``lookup`` sub-command has the usage shown below:

.. code-block:: bash
   Lookup a chemistry in the chemistry registry

   Usage: simpleaf chemistry lookup --name <NAME>

   Options:
     -n, --name <NAME>  the name of the chemistry you wish to lookup
     -h, --help         Print help
     -V, --version      Print version

The single required argument ``--name`` should be the key (name) of an existing chemistry in the current registry. If the key (name) of any chemistry matches, its associated information will be printed.
