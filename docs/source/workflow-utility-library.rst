Simpleaf workflow utility library
===================================

To ease the development of *simpleaf*workflow templates, the *simpleaf* team provides not only some `built-in variables <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=4.%20Utilizing%20built%2Din%20variables%20and%20custom%20library%20search%20paths%20in%20custom%20templates>`_, but also a workflow utility library, which will be automatically passed to the `internal Jsonnet engine <https://github.com/CertainLach/jrsonnet>` of *simpleaf* when parsing a workflow template as the ``__utils`` external variable. One can receive this variable in their templates by adding ``utils=std.extVar("__utils")``, and use the functions in the utility library by calling ``utils.function_name(args)``, where *function_name* should be replaced by an actual function name listed below. To be consistent with the `Jsonnet official documentation <https://jsonnet.org/ref/stdlib.html>`_, here we will list the function signatures with a brief description. One can find the function definitions on `this page <https://github.com/COMBINE-lab/protocol-estuary/blob/main/utils/simpleaf_workflow_utils.libsonnet>`_. 

Terminology
''''''''''''''''''''''''''
- `Simpleaf command record <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=Define%20a%20basic%20workflow%20template>`_: a sub-object in an object that has required identify fields, *Program Name* and *Step*, and the *Program Name* represents one of the simpleaf commands.
- `Recommended main sections <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=2.%20The%20recommended%20layout%20in%20a%20simpleaf%20workflow%20template>`_: the recommended sub-fields of a workflow object.
    - *meta_info*
    - *Recommended Configuration*
    - *Optional Configuration*
    - *External Commands* 
- `Identity fields <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=There%20are%20three%20identity%20fields.>`_: the fields used for identifying a command record.
- A valid field or a valid meta-variable is a field or a meta-variable that exists and is not *null*. 
- A simpleaf flag field is a field in a simpleaf command record that represents one of the `Simpleaf Program Arguments`_.

Frequently Used Functions
'''''''''''''''''''''''''''''''''''''''''''

combine_main_sections(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: An object with the same information and layout as the original object but with the *Recommended Configuration* and *Optional Configuration* sections combined. 

This function combines two `recommended main sections <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=2.%20The%20recommended%20layout%20in%20a%20simpleaf%20workflow%20template>`_, *Recommended Configuration* and *Optional Configuration*, keep all other main sections as it is, and ignore all other sections in the root layer. Those two sections being combined are designed to have an identical layout. If your template contains these two main sections, we recommend applying this function before any other processing steps. However, as this function will ignore all fields that are not simpleaf flag fields in all `command records <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=Define%20a%20basic%20workflow%20template>`_, one should save important non-simpleaf flag fields to other variables before applying this function.

When merging the two main sections, the function will
1. bring all arguments in the nested layers of any simpleaf command record to the same layer as the identity fields of that record live.
2. merge the two sections and bring the subfields of the merged section to the root layer (same layer as these two main sections), and remove the two sections because they are empty.  

add_meta_args(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: an object

**Output**: The same object but with additional ``--threads``, ``--output``, and ``--use-piscem`` fields added to its simpleaf command records if applicable. 

This function finds the meta-variables, if any, defined in the *meta_info* section and the build-in variables passed by *simpleaf* and assigns additional arguments to all qualified simpleaf command records if applicable. One example can be found in our `tutorial <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=workflow%20manifest.-,For%20example,-%2C%20if%20we%20pass>`_. Currently, this function can process three meta-variables:

- *threads*: if the *threads* meta-variable is valid (it exists and is not *null*), all simpleaf command records will be assigned a ``--threads`` field with this value if, for this command, ``--threads`` is a valid argument but it does not exist in the command record.
- *use-piscem*: if the *use-piscem* meta-variable is valid, all simpleaf command records will be assigned a ``--use-piscem`` field with this value if, for this command, ``--use-piscem`` is a valid flag but missing.
-  For *output*, it will first decide the actual output directory: if the *output* meta-variable is valid, this value will be used. Otherwise, the `__output` `built-in variable <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=4.%20Utilizing%20built%2Din%20variables%20and%20custom%20library%20search%20paths%20in%20custom%20templates>`_ will be used. All simpleaf command records will be assigned a ``--output`` field with the actual output directory if, for this command, ``--use-piscem`` is a valid flag but is missing. 

add_index_dir_for_simpleaf_index_quant_combo(o)
"""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: The same object but with an additional ``--index`` field for each qualified *simpleaf quant* command record. 

This function automatically adds the ``--index`` flag field to qualified *simpleaf quant* command records in a workflow object. A qualified *simpleaf quant* command record must have the name *simpleaf_quant* and a corresponding *simpleaf index* command record in the same layer with the name *simpleaf_index*.

This function does the following steps:

1. It traverses the given workflow object to find all fields with a *simpleaf_index* and a *simpleaf_quant* sub-field.
2. For each pair found in step 1, it checks if the *simpeaf_index* field has a ``--output`` valid subfield and if the *simpleaf_quant* field misses the ``--index`` and ``--map-dir`` field. 
3. for each pair satisfied the criteria in step 2, it adds a ``--index`` sub-field to the *simpleaf_quant* field, by appending a */index* to the value of the ``--output`` subfield in *simpleaf_index*. 

For example, if we run the following Jsonnet program,

.. code-block:: console

    local o = {
        "simpleaf_index": {
            "--output": "/path/to/output"
        },
        "simpleaf_quant": {},
        "anohter simpleaf_quant": {},
    };
    utils.add_index_dir_for_simpleaf_index_quant_combo(o)

we will get the following JSON configuration:

.. code-block:: console

    local o = {
        "simpleaf_index": {
            "--output": "/simpleaf/index/output"
        },
        "simpleaf_quant": {
            "--index": "/simpleaf/index/output/index"
        }
        "anohter simpleaf_quant": {},
    };
    utils.add_index_dir_for_simpleaf_index_quant_combo(o)


get(o, f, use_default = false, default = null)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object, f: a field name, use_default: boolean, default: any valid type

**Output**: Return the field if the object has a field with the name indicated by *f*. Otherwise,
  - if *use_default* is *true*, return the value of the *default* argument (defualtly *null*).
  - if *use_default* is false, raise an error.

This function tries to get a field in the provided object and return it. If the field doesn't exist, then it either returns a default value or raises an error.

Simpleaf Program Arguments
''''''''''''''''''''''''''
This section lists the arguments of *simpleaf* command arguments for programs that are supported in *simpleaf workflow*. Usually, these fields are used for obtaining and validating the fields included in a command record. Details about a command record can be found in `protocol estuary <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=There%20are%20three%20identity%20fields.>`_.

utils.SimpleafPrograms["simpleaf index"]
"""""""""""""""""""""""""""""""""""""""""
This field contains all command line flags of the *simpleaf index* command. Furthermore, it also includes the identity fields, *Program Name*, *Step*, and *Active*.

utils.SimpleafPrograms["simpleaf quant"]
"""""""""""""""""""""""""""""""""""""""""
This field contains all command line flags of the *simpleaf quant* command. Furthermore, it also includes the identity fields, *Program Name*, *Step*, and *Active*.

Helper Functions
''''''''''''''''''''''''''''''''''''''''''''

flat_arg_groups(o, path = "")
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: An object with the same information and layout as the original object, but all simpleaf command arguments located at a nested layer of the corresponding simpleaf command record are brought to the same layer as the identity fields of the simpleaf command record. 

The *combine_main_sections* function calls this function internally. When merging the two main sections, the function will bring all arguments in the nested layers of any simpleaf command record to the same layer as the identity fields of that record live. See our example on `setting the path for showing trajectory <https://github.com/COMBINE-lab/protocol-estuary/blob/17bfb476eaf5216f195876e385f19eade37d7dc3/utils/simpleaf_workflow_utils.libsonnet#L292>`_.

recursive_get(o, target_name, path = "")
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object, target_name: name of the field to look for, path: trajectory path to the object if the object lives in a nested layer

**Output**: The value of the target field if it is in the object, else *null*.

This function recursively traverses the object to find the field with the target name. If it finds it, it will return the value of the field. If not, it will return a *null*. See our example on `setting the path for showing trajectory <https://github.com/COMBINE-lab/protocol-estuary/blob/17bfb476eaf5216f195876e385f19eade37d7dc3/utils/simpleaf_workflow_utils.libsonnet#L292>`_.

get_output(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: a string representing the actual output directory.

This function checks two places to decide the output directory and return it as a string.
1. the *__output* built-in variable, which represents the path provided via the ``--output`` argument of ``simpleaf workflow run``.
2. the *output* meta-variable in the *meta_info* main section.

If the meta-variable is valid, it will be the return value of this function. Otherwise, the built-in variable will be the return value. Notice that if a template uses this function to parse the template out of *simpleaf*, for example, using *jsonnet* or *jrsonnet*, one must manually provide the *__output* variable by doing something like ``jsonnet template.jsonnet --ext-code "__output='/path/to/a/directory'"``.

check_invalid_args(o, path = "")
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object, path: trajectory path to the object if the object lives in a nested layer

**Output**: If all simpleaf arguments are valid, the original object will be returned. Otherwise, an error will be raised.

This function traverses the given object to find simpleaf command records. If the records contain invalid fields that neither represents an argument of the simpleaf program nor an identity field, an error will be raised. If no simpleaf command record contains invalid fields, the original object will be returned. However, we do not recommend validating simpleaf commands in any template because when parsing the resulting workflow manifest, simpleaf itself will validate all simpleaf commands and return clear error messages if encountering invalid command records.

get_recommended_args(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: An object with the same information and layout as the original object's *Recommended Configuration* section but contains only the missing fields with a `null`.

This function will recursively traverse the *Recommended Configuration* main section to find all fields with a null value and return those fields as the original layout of *Recommended Configuration*.

get_missing_args(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: o: an object

**Output**: An object with the same layout as the original object but only contains the missing fields with a `null`.

This function will recursively traverse the object to find all fields with a null value and return those fields as the layout of the original object.
