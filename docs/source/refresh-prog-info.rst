``refresh-prog-info`` command
=============================

The ``refresh-prog-info`` command reads the paths stored for the executables used by ``simpleaf`` (specifically those for 
``alevin-fry``, ``piscem`` and ``salmon``), and for each of the installed programs, fetches and updates the associated versions
of the programs.

This command is useful because ``simpleaf`` records the version associated with each of the registered tools when their paths are
first set with the ``--set-paths`` command.  However, if he executable is subsequently updated, the associate version won't be 
revised (since this operation may have been done outside of ``simpleaf``).  This command provides a simple way to ensure that the 
proper version information is associated with each of the programs registered with ``simpleaf``.
