simpleaf workflow patch
=======================

``simpleaf workflow patch`` does awesome stuff.

Full Usage
^^^^^^^^^^

The relevant options (which you can obtain by running ``simpleaf workflow patch -h``) are:

.. code-block:: console

  Patch a workflow template or instantiated manifest with a subset of parameters to produce a series of workflow manifests

  Usage: simpleaf workflow patch --patch <PATCH> <--manifest <MANIFEST>|--template <TEMPLATE>>

  Options:
    -m, --manifest <MANIFEST>  fully-instantiated manifest (JSON file) to patch. If this argument is given, the patch is applied directly 
                               to the JSON file in a manner akin to simple key-value replacement. Since the manifest is fully-instantiated, 
                               no derived values will be affected
    -t, --template <TEMPLATE>  partially-instantiated template (JSONNET file) to patch. If this argument is given, the patch is 
                               applied *before* the template is instantiated (i.e. if you override a variable used elswhere in 
                               the template, all derived values will be affected)
    -p, --patch <PATCH>        patch to apply as a ';' separated parameter table with headers declared as specified in the documentation
    -h, --help                 Print help
    -V, --version              Print version

