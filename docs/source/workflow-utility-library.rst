Simpleaf workflow utility library
===================================

To ease the development of *simpleaf* workflow templates, the *simpleaf* team provides not only some `built-in variables <https://combine-lab.github.io/alevin-fry-tutorials/2023/build-simpleaf-workflow/#:~:text=4.%20Utilizing%20built%2Din%20variables%20and%20custom%20library%20search%20paths%20in%20custom%20templates>`_, but also a workflow utility library, which will be automatically passed to the `internal Jsonnet engine <https://github.com/CertainLach/jsonnet>`_ of *simpleaf* when parsing a workflow template as the ``__utils`` external variable. One can receive this variable in their templates by adding ``utils=std.extVar("__utils")``, and use the functions in the utility library by calling ``utils.function_name(args)``, where *function_name* should be replaced by an actual function name listed below. To be consistent with the `Jsonnet official documentation <https://jsonnet.org/ref/stdlib.html>`_, here we will list the function signatures with a brief description. One can find the function definitions on `this page <https://github.com/COMBINE-lab/protocol-estuary/blob/main/utils/simpleaf_workflow_utils.libsonnet>`_.

Import the utility library
""""""""""""""""""""""""""""""""""""""""""""""

As the built-in variables are provided by *simpleaf* to its internal Jsonnet engine, they will be unavailable if we want to parse the template directly using Jsonnet or Jrsonnet. Therefore, when debugging templates that utilize the utility library with an external Jsonnet engine, we must manually provide the ``__utils`` external variable to Jsonnet and either copy and paste the library file to the same directory as the template file, which is the default library searching path when calling jsonnet, or provide the directory containing the library as an additional library searching path. Although in the following code chunk, we show the code for both ways, we only need to select one in practice. Here we assume that *simpleaf* has been configured correctly, i.e., the ``ALEVIN_FRY_HOME`` environment variable has been set and a local copy of the protocol-estuary exists. If not, one can directly obtain the library file from its GitHub repository.

.. code-block:: shell

    # If we haven't set up simpleaf, we pull the file directly from github
    wget https://github.com/COMBINE-lab/protocol-estuary/blob/main/utils/simpleaf_workflow_utils.libsonnet
    
    # Otherwise, we either copy the library file to the same dir as the template
    copy $ALEVIN_FRY_HOME/protocol-estuary/protocol-estuary-main/utils/simpleaf_workflow_utils.libsonnet .

    # or provide the directory as an additional library searching path via --jpath 
    jsonnet a_template_using_utils_lib.jsonnet --ext-code '__utils=import "simpleaf_workflow_utils.libsonnet"' --jpath "$ALEVIN_FRY_HOME/protocol-estuary/protocol-estuary-main/utils"

where ``--ext-code`` is the flag for passing an external variable, and ``--jpath`` specifies the library searching path.  

Although *simpleaf* automatically provides the utility library as the external variable ``__utils``, we must receive this external variable in our template before starting using the functions provided in this library. 

To do this, we recommend adding the following code at the beginning of your workflow template.

.. code-block:: console

    local utils=std.extVar("__utils");

utils.ref_type(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: 

- ``o``: an object with 
    - a *type* field and 
    - an object field with the name specified by the *type* field. Other fields will be ignored. 
    For example, ``{type: "spliceu", spliceu: {gtf: "genes.gtf", fasta: "genome.fa", "field_being_ignored": "ignore me"}}``.

**Output**: 

- An object with the *simpleaf index* arguments that are related to the specified reference type in the input object.

This function has four modes (reference types), triggered by the ``type`` field in the input object. When specifying a mode, the input object must contain an object field named by that mode and contain the required fields. Otherwise, an error will be raised. The four modes are:

- *spliceu* (*spliced+unspliced* reference): The required fields are:
    - ``gtf``: A string representing the path to a gene annotation GTF file.
    - ``fasta``: A string representing the path to a reference genome FASTA file.
- *splici* (*spliced+intronic* reference): The required fields are:
    - ``gtf``: A string representing the path to a gene annotation GTF file.
    - ``fasta``: A string representing the path to a reference genome FASTA file.
    - ``rlen``: An *optional* field representing the read length in the dataset. If not provided, the default value, 91, will be used.
- *direct_ref*: The required fields are:
    - ``ref_seq``: A string representing the path to a *transcriptome* FASTA file.
    - ``t2g_map``: A string representing the path to a transcript-to-gene mapping file.
- *existing_index*: The required fields are:
    - ``index``: A string representing the path to an existing index directory.
    - ``t2g_map``: A string representing the path to a transcript-to-gene mapping file.

**Wrapper functions**: We also provide separate functions for each of the four modes, ``utils.splici``, ``utils.spliceu``, ``utils.direct_ref``, and ``utils.existing_index``, which are thin wrappers of ``utils.ref_type``. These four functions take an object containing their required fields introduced above.


**Example Usage** 

.. code-block:: jsonnet
    
    # import the utility library
    local utils=std.extVar("__utils");

    local splici_args = {
        gtf : "genes.gtf",
        fasta : "genome.fa",
        rlen : 91
    };

    local ref_type = utils.ref_type({
        type : "splici",
        splici : splici_args
    })

    local splici = utils.splici(splici_args);

In the above example, the objects ``ref_type`` and ``splici`` are identical and look like the following:

.. code-block:: jsonnet

    {   
        # hidden, system fields
        type :: "splici", # hidden field
        arguments :: {gtf : "genes.gtf", fasta : "genome.fa", rlen : 91}, # hidden field
        
        # fields shown in the manifest
        "--ref-type" : "splici",
        "--fasta" : "genome.fa",
        "--gtf" : "genes.gtf",
        "--rlen" : 91
    } 


utils.simpleaf_index(step, ref_type, arguments, output)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**:

- ``step``: An integer indicating the step number (execution order) of this simpleaf command record in the workflow.
- ``ref_type``: A ``ref_type`` object returned by calling ``utils.ref_type`` or any object with the same format.
- ``arguments``: An object in which each field represents a *simpleaf index* argument. Furthermore, there must be a field called ``active`` representing the active state of this `simpleaf index` command.
- ``output``: A string representing the output directory of the `simpleaf index` command.

**Output**:

- A well-defined *simpleaf index* command record.

**Example Usage** 

.. code-block:: jsonnet

    # import the utility library
    local utils=std.extVar("__utils");

    local splici_args = {
        gtf : "genes.gtf",
        fasta : "genome.fa",
        rlen : 91,
    };
    
    local splici = utils.splici(splici_args);

    local arguments = {
        active : true,
        "--use-piscem" : true,
    };
        
    local simpleaf_index = utils.simpleaf_index(
        1, # step number
        splici, # ref_type,
        arguments,
        "./simpleaf_index" # output directory
    );


The ``simpleaf_index`` object in the above code chunk will be  

.. code-block:: jsonnet

    {
        # hidden, system fields
        ref_type :: {}, # hidden field. The actual contents are omitted. see above example code for function `ref_type`
        arguments :: {active : true, "--use-piscem" : true},  # hidden field
        output :: "./simpleaf_index", # hidden field
        index :: "./simpleaf_index/index", # hidden field
        t2g_map :: "./simpleaf_index/index/t2g_3col.tsv", # hidden field

        # fields shown in in the manifest
        program_name : "simpleaf index",
        step : 1,
        active : true,
        "--output": "./workflow_output/simpleaf_index",
        "--gtf" : "genes.gtf",
        "--fasta" : "genome.fa",
        "--rlen" : 91,
        "--use-piscem" : true
    }


utils.map_type(o, simpleaf_index = {})
""""""""""""""""""""""""""""""""""""""""""""""

**Input**:

- ``o``: an object with
    - a ``type`` field, and
    - an object field with the name specified by the ``type`` field. Other fields will be ignored. 
    
    For example, ``{"type": "map_reads", "map_reads": {"reads1": null, "reads2": null}, "field_being_ignored": "ignore me"}``.
- ``simpleaf_index``: An empty object if in `existing_mappings` mode, or the output object of the `simpleaf_index` function if in `map_reads` mode. The default value is an empty object.

**Output**: 

- An object with the `simpleaf quant` arguments that are related to the specified map type in the input object.

This function has two modes (map types), triggered by the `type` field in the input object. When specifying a mode, the input object must contain an object field named by that mode and contain the required fields. Otherwise, an error will be raised. The two modes are:

- `map_reads`: Map reads against the provided index or an index built from a previous step. The required fields are
    - ``reads1``: A string representing the path to a gene annotation GTF file,
    - ``reads2``: A string representing the path to a reference genome FASTA file.
- `existing_mappings`: Skip mapping and use the existing mapping results. The required fields are
    - ``map_dir``: A string representing the path to the mapping result directory,
    - ``t2g_map``: A string representing the path to a transcript-to-gene mapping file.

**Wrapper functions**: We also provide separate functions for each of the two modes, ``utils.map_reads`` and ``utils.existing_mappings``, which are thin wrappers of ``utils.map_type``. These two functions take an object containing their required fields introduced above.

**Example Usage** 

.. code-block:: jsonnet

    # import the utility library
    local utils=std.extVar("__utils");

    local simpleaf_index = {}; # The return of object of simpleaf_index function in its example usage 

    local map_reads_args = {
        reads1 : "reads1.fastq",
        reads2 : "reads2.fastq",
    };

    local map_type = utils.map_type({
        type : "map_reads",
        map_reads : map_reads_args
    });

    local map_reads = utils.map_reads(map_reads_args);

In the above example, the objects ``map_type`` and ``map_reads`` are identical and look like the following:

.. code-block:: jsonnet

    {   
        # hidden, system fields
        type :: "map_reads", # hidden field
        arguments :: {reads1 : "reads1.fastq", reads2 : "reads2.fastq"}, # hidden field
        
        # fields shown in the manifest
        "--index" : "./workflow_output/simpleaf_index/index",
        "--t2g-map": "./workflow_output/simpleaf_index/index/t2g_3col.tsv",
        "--reads1" : "reads1.fastq",
        "--reads2" : "reads2.fastq"
    } 


utils.cell_filt_type(o)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: 

- ``o``: an object with 
    - a ``type`` field, and 
    - an argument field with the name specified by the ``type`` field. Other fields will be ignored. 
    
    For example, ``{"type": "explicit_pl", "explicit_pl": "whitelist.txt", "field_being_ignored": "ignore me"}``

**Output**: 

- An object with the `simpleaf quant` arguments that are related to the specified cell filtering type in the input object.

This function has five modes (cell filtering types), triggered by the `type` field in the input object. When specifying a mode, the input object must contain an object field named by that mode and contain the required fields. Otherwise, an error will be raised. For more details, please refer to the online documentation of `simpleaf quant <https://simpleaf.readthedocs.io/en/latest/quant-command.html>`_ and `alevin-fry <https://alevin-fry.readthedocs.io/en/latest/>`_. The five modes are:

- `unfiltered_pl`: No cell filtering but correcting cell barcodes by an external or default (only works for 10X Chromium V2 and V3). The corresponding argument value field can be ``true`` (using the default whitelist if in `10xv2` and `10xv3` chemistry), or a string representing the path to an unfiltered permit list file.
- `knee`: Knee point-based filtering. The corresponding argument value field must be `true` if selected.
- `forced`: Use a forced number of cells. The corresponding argument field must be an integer representing the number of cells that can pass the filtering.
- `expect`: Use the expected number of cells. The corresponding argument field must be an integer representing the expected number of cells.
- `explicit_pl`: Use a filtered, explicit permit list. The corresponding argument field must be a string representing the path to a cell barcode permit list file.

**Wrapper functions**: We also provide a separate function for each mode, ``utils.unfiltered_pl``, ``utils.knee``, ``utils.forced``, ``utils.expect``, and ``utils.explicit_pl``, which are thin wrappers of ``utils.cell_filt_type``. These functions take an object containing their required fields introduced above.

**Example Usage** 

.. code-block:: jsonnet
    
    # import the utility library
    local utils=std.extVar("__utils");

    local unfiltered_pl_args = {
        unfiltered_pl : true
    };

    local cell_filt_type = utils.cell_filt_type({
        type : "unfiltered_pl",
        unfiltered_pl : unfiltered_pl_args
    })

    local unfiltered_pl = utils.unfiltered_pl(unfiltered_pl_args);

In the above example, the objects `cell_filt_type` and `unfiltered_pl` are identical and look like the following:

.. code-block:: jsonnet

    {   
        # hidden, system fields
        type :: "unfiltered_pl", # hidden field
        arguments :: true, # hidden field
        
        # fields shown in the manifest
        "--unfiltered-pl" : true
    } 

simpleaf_quant(step, map_type, cell_filt_type, arguments, output)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**: 

- ``step`` : An integer indicating the step number (execution order) of this simpleaf command record in the workflow.
- ``map_type`` : A `map_type` object returned by calling `utils.map_type` or any object with the same format. 
- ``cell_filt_type`` : A `cell_filt_type` object returned by calling `utils.cell_filt_type` or any object with the same format. 
- ``arguments`` : an object in which each field represents a `simpleaf quant` argument. Furthermore, there must be a field called ``active`` representing the active state of this simpleaf index command. 
- ``output`` : A string representing the output directory of this `simpleaf quant` command.

**Output**: 

- A well-defined `simpleaf quant` command record.

**Example Usage** 

.. code-block:: jsonnet

    # import the utility library
    local utils=std.extVar("__utils");

    local arguments = {
        active : true,
        "--chemistry" : "10xv3",
        "--resolution" : "cr-like"
    };

    local simpleaf_quant = utils.simpleaf_quant(
        2, # step number
        map_type, # defined in the example usage of function `map_reads`
        cell_filt_type, # defined in the example usage of function `cell_filt_type`
        arguments,
        "./simpleaf_quant" # output directory
    );


The ``simpleaf_quant`` object in the above code chunk will be  

.. code-block:: jsonnet

    {
        # hidden, system fields
        map_type :: {}, # hidden field. The actual contents are omitted. see above example code for function `map_reads`
        cell_filt_type :: {}, # hidden field. The actual contents are omitted. see above example code for function `cell_filt_type`
        arguments :: {active : true, "--chemistry" : "10xv3", "--resolution" : "cr-like"},  # hidden field
        output :: "./simpleaf_quant", # hidden field

        # fields shown in in the manifest
        program_name : "simpleaf index",
        step : 1,
        active : true,
        "--chemistry": "10xv3",
        "--index": "./workflow_output/simpleaf_index/index",
        "--min-reads": 10,
        "--output": "./workflow_output/simpleaf_quant",
        "--reads1": "reads1.fastq",
        "--reads2": "reads2.fastq",
        "--resolution": "cr-like",
        "--t2g-map": "./workflow_output/simpleaf_index/index/t2g_3col.tsv",
        "--unfiltered-pl": true
    }


feature_barcode_ref(start_step, csv, name_col, barcode_col, output)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**:

- ``start_step``: An integer indicating the starting step number (execution order) of the series of command records in the workflow. This function will define three command records with incremental step numbers according to the provided step number.
- ``csv``: A string representing the path to the "feature_barcode.csv" file of the dataset.
- ``name_col``: An integer representing the column index of the feature name column in the feature barcode CSV file.
- ``barcode_col``: An integer representing the column index of the feature barcode sequence column in the feature barcode CSV file.
- ``output``: A string representing the parent output directory of the result files. It will be created if it doesn't exist.

**Output**: 

- An object containing three external command records, including `mkdir`, `create_t2g`, and `create_fasta`, and a hidden object that follows the output format of `utils.ref_type` shown above. This `ref_type` object is of the `direct_ref` type. It can be used as the second argument of `utils.simpleaf_index`. In this `ref_type` object,

This function defines three external command records:

1. `mkdir`: This command calls the `mkdir` shell program to create the output directory recursively if it doesn't exist.
2. `create_t2g`: This command calls `awk` to create a transcript-to-gene mapping TSV file according to the input `csv` file, in which the transcript ID and gene ID of each feature barcode are identical. The expected output file of this command will be named ".feature_barcode_ref_t2g.tsv" and located in the provided output directory.
3. `create_fasta`: This command calls `awk` to create a FASTA file according to the input `csv` file, in which each feature barcode is a FASTA record. The expected output file of this command will be named ".feature_barcode_ref.fa" and located in the provided output directory.

Please note that the `start_step` argument represents the starting step of the series of external commands. If `start_step` is set to 1, then `mkdir` will be assigned step 1, `create_t2g` will be assigned step 2, and so on. Therefore, the step of any future command after the `utils.feature_barcode_ref` commands should not be less than 4.

**Example Usage** 

.. code-block:: jsonnet

    # import the utility library
    local utils=std.extVar("__utils");

    local feature_barcode_ref = utils.feature_barcode_ref(
        1, # start step number
        "feature_barcode.csv", # feature barcode csv
        1, # name_column
        5, # barcode column
        "feature_barcode_ref" # output path
    )

The resulting object will look like the following:

.. code-block:: jsonnet

    {   
        # hidden, system fields
        step :: 1,
        last_step :: 3,
        csv :: "feature_barcode.csv",
        output :: "./feature_barcode_ref",
        ref_seq :: "./feature_barcode_ref/.feature_barcode_ref.fa",
        t2g_map :: "./feature_barcode_ref/.feature_barcode_ref_t2g.tsv",
        
        # external command records
        mkdir : {
            active : true,
            step: step,
            program_name: "mkdir",
            arguments: ["-p", "./feature_barcode_ref"]
        },
        create_t2g : {
            active : true,
            step: step + 1,
            program_name: "awk",
            arguments: ["-F","','","'NR>1 {sub(/ /,\"_\",$1);print $1\"\\t\"$1}'", csv, ">", "./feature_barcode_ref/.feature_barcode_ref_t2g.tsv"]
        },
        
        create_fasta : {
            active : true,
            step: step + 2,
            program_name: "awk",
            arguments: ["-F","','","'NR>1 {sub(/ /,\"_\",$1);print \">\"$1\"\\n\"$5}'", csv, ">", "./feature_barcode_ref/.feature_barcode_ref.fa"]
        },
        ref_type :: {
            type : "direct_ref",
            t2g_map :: "./feature_barcode_ref/.feature_barcode_ref_t2g.tsv",
            "--ref-seq" : "./feature_barcode_ref/.feature_barcode_ref.fa"
        }
    }


barcode_translation(start_step, url, quant_cb, output)
""""""""""""""""""""""""""""""""""""""""""""""

**Input**:

- ``start_step``: An integer indicating the starting step number (execution order) of the series of command records in the workflow. This function will define five command records with incremental step numbers according to the provided step number.
- ``url``: A string representing the downloadable URL to the barcode mapping file. You can use `this URL <https://github.com/10XGenomics/cellranger/raw/master/lib/python/cellranger/barcodes/translation/3M-february-2018.txt.gz>`_ for 10xv3 data.
- ``quant_cb``: A string representing the path to the cell barcode file. Usually, this is at `af_quant/alevin/quants_mat_rows.txt` in the simpleaf quant command output directory.
- ``output``: A string representing the parent output directory of the result files. It will be created if it doesn't exist.

**Output**: 

- An object containing five external command records, including `mkdir`, `fetch_cb_translation_file`, `unzip_cb_translation_file`, `backup_bc_file`, and `barcode_translation`.

This function defines five external command records:

1. `mkdir`: This command calls the `mkdir` shell program to create the output directory recursively if it doesn't exist.
2. `fetch_cb_translation_file`: This command calls `wget` to fetch the barcode mapping file. The expected output file of this command will be called ".barcode.txt.gz", located in the provided output directory.
3. `unzip_cb_translation_file`: This command calls `gunzip` to decompress the barcode mapping file. The expected output file of this command will be called ".barcode.txt", located in the provided output directory.
4. `backup_bc_file`: This command calls `mv` to rename the provided barcode file. The expected output file of this command will have the same path as the provided barcode file but with a `.bkp` suffix.
5. `barcode_translation`: This command calls `awk` to convert the barcodes in the provided barcode file according to the barcode translation file. The expected output file will be put at the provided `quant_cb` path.

Notice that the `start_step` argument represents the starting step of the series of external commands. If `start_step` is set to 1, then `mkdir` will be assigned as step 1, `fetch_cb_translation_file` will be assigned step 2, and so on. Therefore, the step of any future command after the `barcode_translation` commands should not be less than 6.

**Example Usage** 

.. code-block:: jsonnet

    # import the utility library
    local utils=std.extVar("__utils");
    local url = "https://github.com/10XGenomics/cellranger/raw/master/lib/python/cellranger/barcodes/translation/3M-february-2018.txt.gz";
    local quant_cb = "simpeaf_quant/af_quant/alevin/quants_mat_rows.txt";

    local barcode_translation = utils.barcode_translation(
        1, # start step number
        url,
        quant_cb,
        "simpeaf_quant/af_quant/alevin" # output path
    )

The resulting object will look like the following:

.. code-block:: jsonnet

    {
        step :: 1,
        last_step :: 5,
        url :: "https://github.com/10XGenomics/cellranger/raw/master/lib/python/cellranger/barcodes/translation/3M-february-2018.txt.gz",
        quant_cb :: "simpeaf_quant/af_quant/alevin/quants_mat_rows.txt",
        output :: "simpeaf_quant/af_quant/alevin",
        mkdir : {
            active : true,
            step : step,
            program_name : "mkdir",
            arguments : ["-p", "simpeaf_quant/af_quant/alevin"]
        },

        fetch_cb_translation_file : {
            active : true,
            step : step + 1,
            program_name : "wget",
            arguments : ["-O", "simpeaf_quant/af_quant/alevin/.barcode.txt.gz", "https://github.com/10XGenomics/cellranger/raw/master/lib/python/cellranger/barcodes/translation/3M-february-2018.txt.gz"]
        },

        unzip_cb_translation_file : {
            active : true,
            step : step + 2,
            "program_name" : "gunzip",
            "arguments": ["-c", "simpeaf_quant/af_quant/alevin/.barcode.txt.gz", ">", "simpeaf_quant/af_quant/alevin/.barcode.txt"]
        },

        backup_bc_file : {
            active : true,
            step: step + 3,
            program_name: "mv",
            arguments: ["simpeaf_quant/af_quant/alevin/quants_mat_rows.txt", "simpeaf_quant/af_quant/alevin/quants_mat_rows.txt.bkp"]
        },

        // Translate RNA barcode to feature barcode
        barcode_translation : {
            active : true,
            step: step + 4,
            program_name: "awk",
            arguments: ["'FNR==NR {dict[$1]=$2; next} {$1=($1 in dict) ? dict[$1] : $1}1'", "simpeaf_quant/af_quant/alevin/.barcode.txt", "simpeaf_quant/af_quant/alevin/quants_mat_rows.txt.bkp", ">", "simpeaf_quant/af_quant/alevin/quants_mat_rows.txt"]
        },  
    }

utils.get(o, f, use_default = false, default = null)
""""""""""""""""""""""""""""""""""""""""""""""""""""""""""""""

**Input**: 

- ``o``: an object,
- ``f``: the target field name, 
- ``use_default``: boolean, 
- ``default``: a default value returned if the target field doesn't exist.

**Output**: 

- Return the target field *f* in the given object if the object has a sub-field called ``f``. Otherwise,

- if ``use_default`` is ``true``, return the value of the ``default`` argument.
- if ``use_default`` is false, raise an error.

This function tries to (non-recursively) get a sub-field in the provided object and return it. If the field doesn't exist, then it either returns a default value or raises an error.

**Example Usage**

.. code-block:: jsonnet
    
    local utils = std.extVar("__utils");
    
    local splici_args = {
        gtf : "genes.gtf",
        fasta : "genome.fa",
        rlen : 91
    };

    {
        default_behavior : utils.get(splici_args, "gtf"), # this will return "genes.gtf",

        not_exist : utils.get(splici_args, "I do not exist"), # raise error
        
        provide_default : utils.get(splici_args, "I do not exist", true, "but I have a default value") # this yields "but I have a default value"

    }
