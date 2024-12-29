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

.. code-block:: bash

  Add or refresh chemistry definitions from the upstream repository

  Usage: simpleaf chemistry refresh [OPTIONS]

  Options:
    -f, --force    overwrite an existing matched chemistry even if the version isn't newer
    -d, --dry-run  report what would happen with a refresh without actually performing one on the actual chemistry registry
    -h, --help     Print help

This sub-command consults the remote ``simpleaf`` repository to check for an updated chemistry registry, and adds any new chemistries from that registry (or updates the entries for any chemistries in that registry whose version number has increased).  
If the ``dry-run`` flag is passed, the actions to be taken will be printed, but the registry will not be modified. If the ``--force`` command is passed, local chemistry definitions will be overwritten by matching remote definitions, even if the remote
definition has a lower version number.

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
   Usage: simpleaf chemistry remove [OPTIONS] --name <NAME>

   Options:
     -n, --name <NAME>  the name of the chemistry you wish to remove (can be a regex)
     -d, --dry-run      print out the action that would be taken rather than taking it
     -h, --help         Print help
     -V, --version      Print version

The single required argument ``--name`` should be the key (name) of some chemistry in the current registry *or* a regular expression that can be used to match one or more 
chemistries in the registry.  If this chemistry is found, it will be removed from the registry. If the ``--dry-run`` flag is passed, the chemistries to be removed 
will be printed, but no modification of the registry will occur.

``simpleaf chemistry lookup``
----------------------

The ``lookup`` sub-command has the usage shown below:

.. code-block:: bash

  Lookup a chemistry in the chemistry registry

  Usage: simpleaf chemistry lookup --name <NAME>

  Options:
    -n, --name <NAME>  the name of the chemistry you wish to lookup (or a regex for matching chemistry names)
    -h, --help         Print help
    -V, --version      Print version

The single required argument ``--name`` should be the key (name) of a chemistry in the current registry or a regular expression that can match the names of chemistries in the 
registry. If the provided name or regex matches any registered chemistry, its associated information will be printed.

``clean`` sub-command
---------------------

The ``clean`` sub-command has the usage shown below:

.. code-block:: bash
  Search for unused permit lists and remove them from the ALEVIN_FRY_HOME cache

  Usage: simpleaf chemistry clean [OPTIONS]

  Options:
    -d, --dry-run  just show what is to be removed rather than
    -h, --help     Print help
    -V, --version  Print version


There is no required argument.  The sub-command will search for unused permit list files in the ``simpleaf`` permit list directory, and remove them.
If the ``--dry-run`` flag is passed, the names of the files to be removed will be printed, but those files will noe be removed.


``fetch`` sub-command
---------------------

The ``fetch`` sub-command has the usage shown below:

.. code-block:: bash
   
  Download the corresponding permit lists for the chemistry/ies

  Usage: simpleaf chemistry fetch [OPTIONS]

  Options:
    -c, --chemistries <CHEMISTRIES>  a list of chemistries to fetch (or a single regex for matching multiple chemistries)
    -d, --dry-run                    show what will be downloaded without downloading anything
    -h, --help                       Print help
    -V, --version                    Print version


The required ``--chemistries`` argument can be the name of a chemistry, a "," separated list of chemistries, or a (singular) regular expression 
matching the names of multiple chemistries.  The registry will be scanned, and for any chemistry in the requested list of matching the provided
regular expression, the corresponding permit list will be downloaded (unless it is already present).  If the ``--dry-run`` flag is passed, then 
the permit lists that would be fetched will be printed, but none will actually be downloaded.
