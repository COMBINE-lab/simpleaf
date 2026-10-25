Installation
============


``Simpleaf`` can be installed from source, from `crates.io <https://crates.io/crates/simpleaf>`_, or installed via `bioconda <https://bioconda.github.io/recipes/simpleaf/README.html>`_. ``simpleaf`` requires `alevin-fry <https://github.com/COMBINE-lab/alevin-fry>`_, `piscem <https://github.com/COMBINE-lab/piscem>`_, and ``wget``.

.. caution::

  This release requires **piscem >= 0.24.0** and **alevin-fry >= 0.19.0**. These are hard requirements, not recommendations: ``simpleaf set-paths`` will reject older binaries, and ``quant``, ``multiplex-quant`` and ``atac process`` re-check both registered tools (``index`` and ``atac index`` re-check piscem) before any work. Upgrade the two together: piscem 0.24.0 writes **RAD v2**, which alevin-fry 0.18.x cannot read (its ``generate-permit-list`` aborts with an "invalid utf-8 sequence" panic), while alevin-fry 0.19.0 reads both RAD v2 and the RAD v1 written by earlier piscem, including mapped directories passed with ``--map-dir``. piscem 0.24.0 also changes the index format, so **indexes built with an earlier piscem must be rebuilt** with ``simpleaf index``; piscem stops on an old index with a "must be rebuilt" error. Flex probe indexes that ``multiplex-quant`` cached itself under ``$ALEVIN_FRY_HOME/probe_indices`` are rebuilt automatically on first use; delete that directory only if the cache is read-only to you. If you install from bioconda, check that the versions it resolves meet these floors.



Recommended: installing from conda
----------------------------------

We recommend that Linux and macOS users install ``simpleaf`` from bioconda, because all its dependencies are also available on conda, and will be automatically installed (except ``piscem``) when installing ``simpleaf``.

.. code-block:: console

    conda install simpleaf piscem -c bioconda -c conda-forge


**Apple-silicon and ARM users** need no special handling. bioconda ships native ``osx-arm64`` and ``linux-aarch64`` packages for ``simpleaf``, ``piscem``, and ``alevin-fry``, so the command above works as-is in a native shell. Rosetta2 is not required, and installing under emulation is no longer recommended.

Installing with cargo
---------------------

cargo is the rust package manager. ``simpleaf`` is available on `crate.io <https://crates.io/crates/simpleaf>`_ and can be installed from cargo.

.. code-block:: console

    cargo install simpleaf


Once installed, one will need to set the path to the executable of dependencies using the ``simpleaf set-paths`` program as discussed in section `Set Up Simpleaf manually <https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=4.%20Set%20Up%20Simpleaf%20Manually>`_.

Building from source (from GitHub)
----------------------------------

You can also choose to build simpleaf from source by pulling its GitHub repo and build it as a normal rust program. Then, one needs to `set up simpleaf manually <https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=4.%20Set%20Up%20Simpleaf%20Manually>`_.

.. code-block:: console

    git clone https://github.com/COMBINE-lab/simpleaf.git && cd simpleaf
    cargo build --release




