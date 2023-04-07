.. _get workflow config:

``get-workflow-config`` command
===============================

The ``get-workflow-config`` command helps fetch the configuration files of a published simpleaf workflow from the `protocol estuary <hhttps://github.com/COMBINE-lab/protocol-estuary>`_ GitHub repository to a local directory.
It searches the workflow in *protocol estuary* according to the string passed to the ``--name``(or ``-n``) flag, and write all related file to the directory passed to the ``--output`` (or ``-o``) flag. If invoking unpublished workflows one developed locally, one can skip this step and provide the workflow configuration Jsonnet program directly to ``simpleaf workflow`` via the ``--config-path`` flag. Otherwise, one should get the configuration Jsonnet program of a workflow by calling ``get-workflow-config``, fill the required information as described below, and pass the modified Jsonnet program to ``simpleaf workflow`` via the ``--config-path`` flag. 

If the given name is not a valid workflow name, an error will be returned. At the same time, ``simpleaf`` will search for workflows with a similar name and list those workflow names in the error message.

When writing the configuration files to the output directroy, ``get-workflow-config`` will first create a sub-directory in the outpuit directory named as workflow name + _config. This sub-directory should at least contains the workflow configuration Jsonnet program named by the workflow name and ends with `.jsonnet`, and other helping files if any. For example, if we run 

.. code-block:: shell

    simpleaf get-workflow-config --name cite-seq-ADT+HTO_10xv2 -o output_dir
    
The the workflow configuration Jsonnet program will be exported to ``output_dir/cite-seq-ADT+HTO_10xv2_config/cite-seq-ADT+HTO_10xv2.jsonnet``.


Providing information in workflow configuration file
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Usually, a published workflow contains four sections:

1) ``Recommended Simpleaf Configuration``: For most users, this section is the only section needed to be completed, i.e., replacing all ``null`` in the section by a meaningful value. The Jsonnet program should be smart enough to generate a valid workflow description JSON from the information provided here. 
2) ``Optional Simpleaf Configuration``: This section contains advanced options that are not covered in the ``Recommended Simpleaf Configuration`` section. The behavior of the workflow can be finely tuned by providing information in this section.
3) ``External Commands``: This section contains all external shell command records. If you see ``TBD`` in this section, it means that these fields will be filled by the Jsonnet program automatically. 
4) ``meta_info``: This section contains the meta info of this workflow. Sometimes the information provided here is used for controling global arguments, for example the output directory and the number of threads used for each invoked command.

For most users, the ``Recommended Simpleaf Configuration`` is the only section needed to be completed to allow the Jsonnet program to generate a valid workflow description JSON. To fill the missing information, one just needs to replace the ``null`` with a meaningful value. **Notice that** to ease the later parsing process, the values of all command arguments must be provided as strings, i.e., wrapped by quotes (``"value"``), even for integers like the number of threads (for example, ``{“--threads”: "16"}`` for simpleaf commands).

For example, the complete ``meta_info`` section in the configuration Jsonnet program of the ``cite-seq-ADT+HTO_10xv2`` workflow should looks like the following (with all comments removed).

.. code-block:: console

    "meta_info": {
        "template_name":  "CITE-seq ADT+HTO with 10x Chromium 3' v2 (TotalSeq-A chemistry)",
        "template_id": "citeseq_10xv2",
        "template_version": "0.0.1",
        "threads": "16",
        "output": "/path/to/output",
        "use-piscem": false,
    }

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


