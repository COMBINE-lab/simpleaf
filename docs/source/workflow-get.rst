simpleaf workflow get
=====================

The ``simpleaf workflow get`` command helps fetch the files of a registered simpleaf workflow to a local directory. One can run the  :ref:`simpleaf workflow list` command to obtain a list of all available workflows. Please check our tutorial on `running an workflow from an published template <https://combine-lab.github.io/alevin-fry-tutorials/2023/running-simpleaf-workflow/>`_ and `developing custom template from scratch <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/>`_

It searches the workflow registry according to the string passed to the ``--name`` (or ``-n``) flag, pack all related files into a folder named by the workflow name plus a ``_template`` and dump the folder in the directory passed to the ``--output`` (or ``-o``) flag. If invoking local workflows, one can skip this step and provide the workflow template directly to :ref:`simpleaf workflow run`. For a registered workflow, one should get the workflow template from ``simpleaf workflow get``, fill in the required information, and provide the filled template to :ref:`simpleaf workflow run` via the ``--template`` flag. 

If the given name is not a valid workflow name, an error will be returned. At the same time, ``simpleaf`` will search for workflows with a similar name and list those names in the error message.
 
In the template folder dumpped by ``simpleaf workflow get``, the workflow template is named by the workflow name and ends with `.jsonnet`. There might be other library files or log files in the folder, depending on the specific workflow. For example, to pull the workflow for analyzing CITE-seq data, we can do


.. code-block:: shell

    simpleaf workflow get --name cite-seq-ADT+HTO_10xv2 -o output_dir
    
The the workflow template will be exported to ``output_dir/cite-seq-ADT+HTO_10xv2_template/cite-seq-ADT+HTO_10xv2.jsonnet``.


Providing information in workflow configuration file
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Usually, a published workflow contains four sections:

1) ``fast_config``: For most users, this section is the only section needed to be completed, i.e., replacing all ``null`` in the section by a meaningful value. The Jsonnet program should be smart enough to generate a valid workflow description JSON from the information provided here. 
2) ``advanced_config``: This section contains advanced options that are not covered in the ``fast_config`` section. The behavior of the workflow can be finely tuned by providing information in this section.
3) ``external_commands``: This section contains all external shell command records. If you see ``TBD`` in this section, it means that these fields will be filled by the Jsonnet program automatically. 
4) ``meta_info``: This section contains the meta info of this workflow. Sometimes the information provided here is used for controling global arguments, for example the output directory and the number of threads used for each invoked command.

For most users, the ``fast_config`` is the only section needed to instantiate the template. To fill the missing information, one just needs to replace the ``null`` with a meaningful value. For more details, please check out dedicated tutorial on `running workflows from an published template <https://combine-lab.github.io/alevin-fry-tutorials/2023/running-simpleaf-workflow/>`_.


Full Usage
^^^^^^^^^^

The relevant options (which you can obtain by running ``simpleaf workflow get -h``) are:

.. code-block:: console

    Get the workflow template and related files of a registered workflow

    Usage: simpleaf workflow get --name <NAME> --output <OUTPUT>

    Options:
      -o, --output <OUTPUT>  path to dump the folder containing the workflow related files
      -n, --name <NAME>      name of the queried workflow
      -h, --help             Print help
      -V, --version          Print version



