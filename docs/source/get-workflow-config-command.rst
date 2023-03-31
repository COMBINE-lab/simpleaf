.. _get workflow config:

``get-workflow-config`` command
===============================

The ``get-workflow-config`` command helps fetch the configuration files of a published simpleaf workflow from the [protocol estuary GitHub repository](https://github.com/COMBINE-lab/protocol-estuary) to a local directory.
It searches the workflow from ``protocol estuary`` according to the string passed to the ``--name``(or ``-n``) flag, and write all related file to the directory passed to the ``--output`` (or ``-o``) flag. If invoking unpublished workflows one developed locally, one can skip this step and provide the workflow configuration files directly to ``simpleaf workflow`` via the \texttt{--config-file} flag. Otherwise, one should get the workflkow configuration Jsonnet file of a workflow by running ``get-workflow-config``, fill the required information as described below, and pass the modified Jsonnet file to ``simpleaf workflow`` via the ``--config-file`` flag. 

If the given name is not a valid workflow name, an error will be returned. At the same time, ``simpleaf`` will search for workflows with a similar name and list those workflow names in the error message. 

Providing information in workflow configuration file
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Usually, a published workflow contains four sections:

1) ``Recommended Simpleaf Configuration``: For most users, this section is the only section needed to be completed, i.e., replacing all ``null`` value in the section by a meaningful value. The Jsonnet program is smart enough to generate a valid workflow description JSON from the information provided here. 
2) ``Optional Simpleaf Configuration``: This section contains the simpleaf flags that are not covered in the ``Recommended Simpleaf Configuration`` section for each command record. Advanced users can adjust the behaviors of the workflow by providing optional flags.
3) ``External Commands``: This section contains the external shell command records. If you see ``TBD`` in this section, it means that these fields will be filled by the Jsonnet program automatically. 
4) ``meta_info``: This section contains the meta info of this workflow. The information recorded in this section will only be used for logging.

For most users, the ``Recommended Simpleaf Configuration`` is the only section needed to be completed to allow ``simpleaf workflow`` generate a valid workflow description JSON. To fill the missing information, one just needs to replace the ``null``s in with a meaningful value. To ease the later parsing process, the values of all command arguments must be provided as strings, i.e., wrapped by quotes (``"value"``), even for integers like the number of threads (for example, ``{“--threads”: "16"}`` for simpleaf commands).

The relevant options (which you can obtain by running ``simpleaf get-workflow-config -h``) are:

.. code-block:: console

    get the workflow configuration files of a published workflow from protocol estuary
    (https://github.com/COMBINE-lab/protocol-estuary)

    Usage: simpleaf get-workflow-config --name <NAME> --output <OUTPUT>

    Options:
    -h, --help     Print help
    -V, --version  Print version

    Get Config Files:
    -o, --output <OUTPUT>  path to output configuration file, the directory will be created if it doesn't exist
    -n, --name <NAME>      name of the queried workflow


