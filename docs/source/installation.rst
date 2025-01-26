Installation
============


``Simpleaf`` can be installed from source, from `crates.io <https://crates.io/crates/simpleaf>`_, or installed via `bioconda <https://bioconda.github.io/recipes/simpleaf/README.html>`_. ``simpleaf`` `alevin-fry <https://github.com/COMBINE-lab/alevin-fry>`_, and either `piscem <https://github.com/COMBINE-lab/piscem>`_ or `salmon <https://github.com/COMBINE-lab/salmon>`_ (or both if you prefer), as well as ``wget``.



Recommended: installing from conda
----------------------------------

We recommend all x86 (Linux or Mac) users to install ``simpleaf`` from bioconda, because all its dependencies are also available on conda, and will be automatically installed (except ``piscem``) when installing ``simpleaf``.

.. code-block:: console

    conda install simpleaf piscem -c bioconda -c conda-forge


**For Apple-silicon computers**, for example those with an Apple M-series chip, simpleaf should be installed under the x86 emulation layer, in other words, in shell with Rosetta2 enabled. See `this <https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=Attention%20Apple%20silicon%20computer%20users%3A>`_ for details. Furthermore, if one would like to use ``piscem`` on apple silicon, one has to either download the `pre-built piscem executable <https://github.com/COMBINE-lab/piscem/releases>`_ or build piscem from source **in the native shell (with Rosetta2 disabled)** using the commands described `here <https://github.com/COMBINE-lab/piscem#building>`_. Then, piscem can be executed from both Rosetta2 enabled and disabled shell.

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





