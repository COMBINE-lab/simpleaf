# simpleaf

A rust framework to make using alevin-fry _even_ simpler.

## Using simpleaf

  The `simpleaf` program is intended to simply the running of `alevin-fry` in common usage scenarios.  By limiting some of the different options that can be set, it provides a streamlined way to build the splici reference and index in a single command, as well as to process an experiment from raw FASTQ files to a count matrix in a single command.
  
  To work properly, `simpleaf` has a few requirements. Specifically, you should have `pyroe` (>=0.6.2), `salmon` (>=1.5.1), and `alevin-fry` (>=0.6.0) installed.  These can either simply be in your `PATH` variable, or you can explicily provide the path to them using the `set-paths` command of `simpleaf`, which will then cache them in a `JSON` file in your `ALEVIN_FRY_HOME` directory.  Additionally, `simpleaf` requires the following environment variable to be present when it is executed :
  
   * `ALEVIN_FRY_HOME` **REQUIRED** — This directory will be used for persistent configuration and small file (<1G) storage between runs.  If you provide a directory and it doesn't exist, it will be created.  It is easiest to just set this in your enviornment globally so that the same home can be used over many runs without you having to provide the variable explicitly each time.  A good choice for this variable might be something like `~/.alevin_fry_home`.
  
  The `simpleaf` script has three sub-commands:
  
  * `set-paths` — The `set-paths` command will set the paths to the relevant executables and store them in a configuration file in the `ALEVIN_FRY_HOME` directory. If you don't provide an explicit path for a program, `simpleaf` will look in your `PATH` for a compatible version.  This command takes the following optional arguments:
  
```{bash}
USAGE:
    simpleaf set-paths [OPTIONS]

OPTIONS:
    -a, --alevin-fry <ALEVIN_FRY>    path to alein-fry to use
    -h, --help                       Print help information
    -p, --pyroe <PYROE>              path to pyroe to use
    -s, --salmon <SALMON>            path to salmon to use
```
 
  * `index` — The `index` command will take a reference genome FASTA and GTF as input, build a splici reference using the `build_splici_ref.R` script, and then build a sparse `salmon` index on the resulting reference. **Note**: The `index` command requires the `Rscript` executable to be in the path, as well as all of theR packages that are required by `build_splici_ref.R`. The relevant options (which you can obtain by running `./simpleaf index -h`) are:
  
  ```{bash}
USAGE:
    simpleaf index [OPTIONS] --fasta <FASTA> --gtf <GTF> --rlen <RLEN> --output <OUTPUT>

OPTIONS:
    -d, --dedup                    deduplicate identical sequences inside the R script when building the splici reference
    -f, --fasta <FASTA>            reference genome
    -g, --gtf <GTF>                reference GTF file
    -h, --help                     Print help information
    -o, --output <OUTPUT>          path to output directory (will be created if it doesn't exist)
    -p, --sparse                   if this flag is passed, build the sparse rather than dense index for mapping
    -r, --rlen <RLEN>              the target read length the index will be built for
    -s, --spliced <SPLICED>        path to FASTA file with extra spliced sequence to add to the index
    -t, --threads <THREADS>        number of threads to use when running [default: min(16, num cores)]" [default: 16]
    -u, --unspliced <UNSPLICED>    path to FASTA file with extra unspliced sequence to add to the index
  ```
  
   * `quant` — The `quant` command takes as input the index, reads, and relevant information about the experiment (e.g. chemistry), and runs all of the steps of the `alevin-fry` pipeline, from mapping with `salmon` through `quant` with `alevin-fry`. The relevant options (which you can obtain by running `simpleaf quant -h`) are:
  
  ```{bash}
 USAGE:
    simpleaf quant [OPTIONS] --index <INDEX> --resolution <RESOLUTION> --chemistry <CHEMISTRY> --t2g-map <T2G_MAP> --output <OUTPUT> <--knee|--unfiltered-pl|--forced-cells <FORCED_CELLS>|--expect-cells <EXPECT_CELLS>>

OPTIONS:
    -1, --reads1 <READS1>                path to read 1 files
    -2, --reads2 <READS2>                path to read 2 files
    -c, --chemistry <CHEMISTRY>          chemistry
    -e, --expect-cells <EXPECT_CELLS>    use expected number of cells
    -f, --forced-cells <FORCED_CELLS>    use forced number of cells
    -h, --help                           Print help information
    -i, --index <INDEX>                  path to index
    -k, --knee                           use knee filtering mode
    -m, --t2g-map <T2G_MAP>              transcript to gene map
    -o, --output <OUTPUT>                output directory
    -r, --resolution <RESOLUTION>        resolution mode [possible values: cr-like, cr-like-em, parsimony, parsimony-em, parsimony-gene, parsimony-gene-em]
    -t, --threads <THREADS>              number of threads to use when running [default: min(16, num cores)]" [default: 16]
    -u, --unfiltered-pl                  use unfiltered permit list
    -x, --explicit-pl <EXPLICIT_PL>      use a filtered, explicit permit list
  ```
