``set-paths`` command
=====================

The ``set-paths`` command is used to set the paths to the relevant executables and store them in a configuration file in the ``ALEVIN_FRY_HOME`` directory. If you don't provide an explicit path for a program, ``simpleaf`` will look in your ``PATH`` for a compatible version.  Once paths are set with this command, they are cached in a file in the ``ALEVIN_FRY_HOME`` directory, and used to execute other commands in ``simpleaf``. If you wish to update the paths, you can run this command again, and it will _overwrite_ this cache. This command takes the following optional arguments:
  
.. code-block:: bash 

  USAGE:
      simpleaf set-paths [OPTIONS]

  OPTIONS:
      -a, --alevin-fry <ALEVIN_FRY>    path to alein-fry to use
      -h, --help                       Print help information
      -p, --pyroe <PYROE>              path to pyroe to use
      -s, --salmon <SALMON>            path to salmon to use
