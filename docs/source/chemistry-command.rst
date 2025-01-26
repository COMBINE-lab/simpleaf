``chemistry`` command
=====================

The ``chemistry`` command provides functionality to manage and inspect custom chemistries in ``simpleaf``'s registry of recognized custom chemistries. It supports the following operations:

- Add new custom chemistries.
- Remove existing custom chemistries.
- Add or refresh chemistry definitions from the upstream repository.
- Lookup details of a specific chemistry.
- Download corresponding permit lists for chemistries.
- Search for unused permit lists and remove them from the cache.

.. code-block:: console

  operate on or inspect the chemistry registry

  Usage: simpleaf chemistry <COMMAND>

  Commands:
    refresh  Add or refresh chemistry definitions from the upstream repository
    add      Add a new chemistry to the registry of custom chemistries
    remove   Remove a chemistry from the chemistry registry
    clean    Search for unused permit lists and remove them from the ALEVIN_FRY_HOME cache
    lookup   Lookup a chemistry in the chemistry registry
    fetch    Download the corresponding permit lists for the chemistry/ies
    help     Print this message or the help of the given subcommand(s)

  Options:
    -h, --help     Print help
    -V, --version  Print version

These sub-commands are described below.

``simpleaf chemistry refresh``
-----------------------

The ``refresh`` sub-command takes no *required* arguments; it's usage is shown below:

.. code-block:: console

  Add or refresh chemistry definitions from the upstream repository

  Usage: simpleaf chemistry refresh [OPTIONS]

  Options:
    -f, --force    overwrite an existing matched chemistry even if the version is not newer
    -d, --dry-run  report what would happen with a refresh without actually performing one on the actual chemistry registry
    -h, --help     Print help

This sub-command consults the remote ``simpleaf`` GitHub repository to check for updates to the local chemistry registry. It adds any new chemistries from the remote or updates entries for existing chemistries if their version number has increased.

If the ``dry-run`` flag is passed, the actions to be taken will be printed, but the registry will not be modified. If the ``--force`` command is passed, local chemistry definitions will be overwritten by matching remote definitions, even if the remote definition has a lower version number.

``simpleaf chemistry add``
--------------------------

The ``add`` sub-command has the usage shown below:

.. code-block:: console

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


This command allows the user to register a new chemistry or modify an existing one. Once a chemistry is registered, ``simpleaf`` can lookup information about this chemistry when other commands are invoked, eliminating the need to repeatedly pass potentially lengthy command-line flags for this chemistry in the future.

Every chemistry added to the registry has three mandatory properties: ``name``, ``geometry``, and ``expected-ori``.


- ``name``: A unique name (within the existing registry) of the chemistry. It must be a valid UTF-8 identifier. If the name is already registered, the existing definition will be updated if a higher ``--version`` is provided (see below for details). Otherwise, simpleaf will complain and fail.
- ``geometry``: The geometry specification must be provided as a quoted string, and must follow the `Sequence Fragment Geometry Description Language <https://hackmd.io/@PI7Og0l1ReeBZu_pjQGUQQ/rJMgmvr13>`_ as used in the `quant command <https://simpleaf.readthedocs.io/en/latest/quant-command.html#a-note-on-the-chemistry-flag>`_. 
- ``expected-ori``: The expected orientation of the chemistry. It must be one of the following: fw (forward), rc (reverse complement), or both (both orientations). It describes the expected orientation relative to the first (most upstream) mappable biological sequence.
Imagine we have reads from 10x Chromium 5' protocols with read1s and read2s both of 150 base pairs. With this specification, a read1, which is in the forward orientation, contains, from 5' to 3', a cell barcode, a UMI, a fixed fragment, and a fragment representing the 5' end of the cDNA. A read2, which is in the reverse complementary orientation, contains the second (downstream) cDNA fragment relative to its read1. You can find a detailed explanation of the 10x Chromium 5' protocol from `Single Cell Genomics Library Structure <https://teichlab.github.io/scg_lib_structs/methods_html/10xChromium5.html>`_.
If we map the biological sequence in read1s and read2s as paired-end reads (currently only supported when using the default mapper -- piscem), as biological read1s are the first mappable sequences, the expected orientation for this chemistry should be ``fw``, the orientation of read1s. However, if we only map read2s, the expected orientation should be ``rc``, because read2s are the first mappable sequences and are in the reverse complementary orientation.

In addition to the required fields, there are 3 optional fields, as described below. A permit list file must be a TSV file without a header, and the first column must contain the sequence of permitted cell barcodes, i.e., the whitelist of cell barcodes.

- ``local-url``: A fully-qualified path to a file containing the permit list.
- ``remote-url``:  A remote URL providing a location from which a permit list can be downloaded.
- ``version``: A `semver <https://semver.org/>`_ format version tag, e.g., `0.1.0`, indicating the version of the chemistry definition. It is NOT the version or revision of the physical chemistry itself, e.g., as the V2 or V3 in chromium V2 or chromium V3.

**Note** any file provided via the ``local-url`` will be *copied* into the ``ALEVIN_FRY_HOME`` directory. To avoid this copying, for example when you have an extremely large file, you can provide the file directly to the simpleaf commands that take the file, for example, ``simpleaf quant -u /path/to/your/large/permit/list/file``.

``simpleaf chemistry remove``
-----------------------------

The ``remove`` sub-command has the usage shown below:

.. code-block:: console

   Remove a chemistry from the chemistry registry
   Usage: simpleaf chemistry remove [OPTIONS] --name <NAME>

   Options:
     -n, --name <NAME>  the name of the chemistry you wish to remove (can be a regex)
     -d, --dry-run      print out the action that would be taken rather than taking it
     -h, --help         Print help
     -V, --version      Print version

The single required argument ``--name`` should be the key (name) of a chemistry in the current registry or a regular expression that matches the name of one or more chemistries in the registry. If one or more chemistries match, they will be removed from the registry. If the ``--dry-run`` flag is passed, the chemistries to be removed will be printed, but no modification of the registry will occur.

``simpleaf chemistry lookup``
-----------------------------

The ``lookup`` sub-command has the usage shown below:

.. code-block:: console

  Lookup a chemistry in the chemistry registry

  Usage: simpleaf chemistry lookup --name <NAME>

  Options:
    -n, --name <NAME>  the name of the chemistry you wish to lookup (or a regex for matching chemistry names)
    -h, --help         Print help
    -V, --version      Print version

The single required argument ``--name`` should be the key (name) of a chemistry in the current registry or a regular expression that matches the name of one or more chemistries in the registry. If the provided name or regex matches any registered chemistry, its associated information will be printed.

``clean`` sub-command
---------------------

The ``clean`` sub-command has the usage shown below:

.. code-block:: console

  Search for unused permit lists and remove them from the ALEVIN_FRY_HOME cache

  Usage: simpleaf chemistry clean [OPTIONS]

  Options:
    -d, --dry-run  just show what is to be removed rather than
    -h, --help     Print help
    -V, --version  Print version


There is no required argument. The sub-command will search for permit list files in the ``simpleaf`` permit list directory that do not match any registered chemistry, and remove them.
If the ``--dry-run`` flag is passed, the names of the files to be removed will be printed, but those files will not be removed.


``fetch`` sub-command
---------------------

The ``fetch`` sub-command has the usage shown below:

.. code-block:: console
   
  Download the corresponding permit lists for the chemistry/ies

  Usage: simpleaf chemistry fetch [OPTIONS]

  Options:
    -c, --chemistries <CHEMISTRIES>  a list of chemistries to fetch (or a single regex for matching multiple chemistries)
    -d, --dry-run                    show what will be downloaded without downloading anything
    -h, --help                       Print help
    -V, --version                    Print version

The required ``--chemistries`` argument can be the name of a single chemistry, a comma-separated (``,``) list of chemistries' names, or a regular expression matching the names of multiple chemistries. The registry will be scanned, and for any chemistry in the requested list or matching the provided regular expression, the corresponding permit list file(s) will be downloaded unless they are already present.

If the --dry-run flag is passed, the permit list file(s) that would be fetched will be printed, but no files will actually be downloaded.
