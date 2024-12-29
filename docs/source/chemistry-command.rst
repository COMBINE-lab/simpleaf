``chemistry`` command
=====================

The ``chemistry`` command allows operation on (e.g. adding or removing) custom chemistries to ``simpleaf``'s registry of recognized chemistries, and also alows 
inspecting the information associated with a specific chemistry. The command currently has 4 sub-commands ``add``, ``remove``, ``refresh``, and ``lookup``.  

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

These sub-commands are dscribed below.

``refresh`` sub-command
-----------------------

The ``refresh`` sub-command takes no *required* arguments; it's usage is shown below:

.. code-block:: bash

  Add or refresh chemistry definitions from the upstream repository

  Usage: simpleaf chemistry refresh [OPTIONS]

  Options:
    -f, --force    overwrite an existing matched chemistry even if the version isn't newer
    -d, --dry-run  report what would happen with a refresh without actually performing one on the actual chemistry registry
    -h, --help     Print help

This sub-command consults the remote ``simpleaf`` repository to check for an updated chemistry registry, and adds any new chemistries from that registry (or updates the entries for any chemistries in that registry whose version number has incresed).  
If the ``dry-run`` flag is passed, the actions to be taken will be printed, but the registry will not be modified. If the ``--force`` command is passed, local chemistry definitions will be overwritten by matching remote definitions, even if the remote
definition has a lower version number.

``add`` sub-command
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


This command allows the user to register a new chemistry with ``simpleaf``.  Once a chemisty is registered, then ``simpleaf`` will be able to lookup certain information about this chemistry when other commands are invoked, allowing the user to avoid having to pass potentially long command-line flags in future invocations.

Every chemistry added to the registry has 2 mandatory associated properties: a ``name`` and a ``geometry`` specification. The name must be a unique (within the existing registry) name, and a valid UTF-8 identifier. This geometry specification should be provided enclosed in quotes, an in the `same format <https://simpleaf.readthedocs.io/en/latest/quant-command.html#a-note-on-the-chemistry-flag>`_ as would be provided to the ``quant`` command.

In addition to the required fields, there are 4 optional fields: ``expected-ori`` (an expected mapping orientation for reads generated with this chemistry), ``local-url`` a fully-qualified path to a file containing the permit list (i.e. whitelist) for this chemistry (if one exists), ``remote-url`` a remote URL providing a location from which this permit list can be downloaded and ``version`` a version tag you wish to specify along with this chemistry

**Note** any file provided via the ``local-url`` will be *copied* into a subdirectory of the ``ALEVIN_FRY_HOME`` directory. Also, note that the version flag here is **not** meant to specify the version or revision of the physical chemistry itself (e.g. as the V2 or V3 in chromium V2 or chromium V3), but rather is a `semver <https://semver.org/>`_ format tag that will be used for interal tracking purposes (e.g. you will bump this version if you wish to update the chemistry in the registry).


``remove`` sub-command
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

The single required argument ``--name`` should be the key of some chemistry in the current registry *or* a regular expression that can be used to match one or more 
chemistries in the registry.  If this chemistry is found, it will be removed from the registry. If the ``--dry-run`` flag is passed, the chemistries to be removed 
will be printed, but no modification of the registry will occur.

``lookup`` sub-command
----------------------

The ``lookup`` sub-command has the usage shown below:

.. code-block:: bash

  Lookup a chemistry in the chemistry registry

  Usage: simpleaf chemistry lookup --name <NAME>

  Options:
    -n, --name <NAME>  the name of the chemistry you wish to lookup (or a regex for matching chemistry names)
    -h, --help         Print help
    -V, --version      Print version

The single required argument ``--name`` should be the key of some chemistry in the current registry or a regular expression that can match the names of chemistries in the 
registry .  If this chemistry (or any chemistry matching this regex) is found, its associated information will be printed.


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
