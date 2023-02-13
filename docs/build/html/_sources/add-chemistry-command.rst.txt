``add-chemistry`` command
=========================

The ``add-chemistry`` command simply allows adding a new custom chemistry to ``simpleaf``'s registry of recognized chemistries. The sub-command takes a ``--name`` 
(a string you wish to use to designate this chemistry), and a ``--geometry`` (a custom geometry string to which you want to map this chemistry).  The usage is 
as below. Note, if you attempt to add a chemistry for a name that already exists in the custom chemistry registry, the new geometry will overwrite the existing
one. For more details on the syntax used to describe custom geometries, see the relevant documentation on the `custom chemistry specification <https://simpleaf.readthedocs.io/en/latest/quant-command.html#a-note-on-the-chemistry-flag>`_.

.. code-block:: bash

  USAGE:
      simpleaf add-chemistry --name <NAME> --geometry <GEOMETRY>

  OPTIONS:
      -g, --geometry <GEOMETRY>    the geometry to which the chemistry maps
      -h, --help                   Print help information
      -n, --name <NAME>            the name to give the chemistry

