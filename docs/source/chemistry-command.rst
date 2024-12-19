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
      lookup   Lookup a chemistry in the chemistry registry
      help     Print this message or the help of the given subcommand(s)

    Options:
      -h, --help     Print help
      -V, --version  Print version

These sub-commands are dscribed below.

``refresh`` sub-command
-----------------------

The ``refresh`` sub-command takes no arguments, it consults the remote ``simpleaf`` repository to check for an updated chemistry registry, and adds any new chemistries from that registry (or updates the entries for any chemistries in that registry whose version number has incresed).  

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

    Usage: simpleaf chemistry remove --name <NAME>

    Options:
      -n, --name <NAME>  the name of the chemistry you wish to remove
      -h, --help         Print help
      -V, --version      Print version

The single required argument ``--name`` should be the key of some chemistry in the current registry.  If this chemistry is found, it will be removed from the registry.

``lookup`` sub-command
----------------------

The ``lookup`` sub-command has the usage shown below:

.. code-block:: bash
   Lookup a chemistry in the chemistry registry

   Usage: simpleaf chemistry lookup --name <NAME>

   Options:
     -n, --name <NAME>  the name of the chemistry you wish to lookup
     -h, --help         Print help
     -V, --version      Print version

The single required argument ``--name`` should be the key of some chemistry in the current registry.  If this chemistry is found, its associated information will be 
printed.
