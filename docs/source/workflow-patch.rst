simpleaf workflow patch
=======================

The ``simpleaf workflow patch`` command allow "patching" ``simpleaf`` workflows. Specifically, it allows one to patch either a *workflow template*
prior to instantiation (and therefore, to patch the values of variables in the workflow that may affect large parts of the configuration) or
a *workflow manifest* (where patching only directly affects the specific fields being replaced).  Here, the act of patching refers to the 
replacement of one or more fields in the template or manifest with alternative values drawn from a parameter table (e.g. a "sample sheet").

The patch command is useful when you wish to use the "skeleton" of a workflow (e.g. a template with many of the variables set), but you wish to 
parameterize other fields over some set of different options.  

Concretely, for example, you may have many gene 10x chromium v3 samples, all of which you wish to process with 
the same workflow, but providing different reads as input and different output locations for the workflow output.  The ``patch`` command 
makes this easy to accomplish.

When operating on a template, the patch command takes as input a workflow template (which can be uninstantiated or partially filled in) via the ``--template`` 
parameter, as well as a parameter table as a ``;`` separated CSV file via the ``--patch`` parameter (see details on the format `below <#patch-file>`_).  
For each (non-header) row in the CSV file, it will patch the template with parameters provided in this row, instantiate a new manifest from this template (*after* replacement), and 
write the instantiated manifest out to a ``JSON`` file.

When operating on a manifest, the patch command takes as input a manifest files via the ``--manifest`` 
parameter, as well as a parameter table as a ``;`` separated CSV file via the ``--patch`` parameter.  
For each (non-header) row in the CSV file, it will patch the manifest with parameters provided in 
this row and write the resulting patched manifest out to a ``JSON`` file. Note that, in this case, 
since the input being patched is a fully-instantiated manifest, the patch simply replaces the values 
of the designated fields, but it will not affect the values of any fields that are not diretly patched.


Patch file
~~~~~~~~~~

The patch file should be a ``;``-separated CSV-like file.  The header column will contain one entry for each field of the template or manifest
that is to be replaced, as well as an additional special column called ``name``, that gives a name to the parameter tuple encoded in each row.
To refer to a field in the template or manifest, one should use the JSON pointer syntax described in `RFC6901 <https://datatracker.ietf.org/doc/html/rfc6901>`_.
Additionally, since the values being replaced can be of distinct valid JSON types, this type information must also be encoded in the column header.

For example, imagine that your template has two fields defined as below:

.. code-block:: javascript

  {
     "workflow" : {
       /* possibly other content */
       "simpelaf_quant" : {
         /* possibly other content */
         "--reads1" : "reads0_1.fq.gz",
          /* possibly other content */
         "ready" : false,
       }
       /* possibly other content */
     }
  }


that you wish to replace. You wish to replace "--reads1" with "reads1_sample2_1.fq.gz" and "ready" with ``true`` (the boolean value true, not the string).
Then the definition of the corresponding column headers would be ``/workflow/simpleaf_quant/--reads1`` and ``<b>/workflow/simpleaf_quant/ready``, respectively.
The ``<b>`` before the second column header designates that this column will hold boolean parameters.  You could also prefix the first column header 
with ``<s>`` (for the string type), but this is the default and can be omitted.  Finally, then, the full patch file might look something like:

.. code-block:: console

   name;/workflow/simpleaf_quant/--reads1;<b>/workflow/simpleaf_quant/ready
   sample1;"reads1_sample2_1.fq.gz";true

The valid type tags are: 

``<s>`` 
  prefixes a header that is a pointer to a string-valued field. This is also the default
  so if a header has no prefix, then ``<s>`` is implicitly assumed

``<b>`` 
  prefixes a header that is a pointer to a boolean-valued field. 

``<a>`` 
  prefixes a header that is a pointer to an array-valued field. 

**Note**: Currently, patching does not support parameter entries that are
themselves full object types.  Finally, if any entry contains the string "null", 
it will automatically be converted into the JSON ``null`` constant 
(which means that currently, at least, there is a restriction that the 
string "null" can not be patched into a template or manifest).

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

