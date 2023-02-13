# simpleaf

A rust framework to make using alevin-fry _even_ simpler. `simpleaf` encapsulates the process of creating an expanded reference for quantification into a single command (`index`) and the quantification of a sample into a single command (`quant`).

A end-to-end example showing the usage of simpleaf can be found in the tutorial [Generating a scRNA-seq count matrix with simpleaf](https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem). In the same tutorial we also showed how to run `simpleaf` from the [usefulaf](https://hub.docker.com/r/combinelab/usefulaf/tags) dock/singularity image.

Check out the full documentation [here](https://simpleaf.readthedocs.io/en/latest/).

## Installation
The `simpleaf` program can be installed from source, from [crates.io](https://crates.io/crates/simpleaf), or installed via [bioconda](https://bioconda.github.io/recipes/simpleaf/README.html). `simpleaf` requires [`pyroe`](https://github.com/COMBINE-lab/pyroe), [`alevin-fry`](https://github.com/COMBINE-lab/alevin-fry), and either [`piscem`](https://github.com/COMBINE-lab/piscem) or [`salmon`](https://github.com/COMBINE-lab/salmon) (or both if you prefer), as well as `wget`.



### Recommended: installing from conda

We recommend installing `simpleaf` from conda, because all its dependencies are in conda, and will be automatically installed when installing `simpleaf`.

```shell
conda install simpleaf -c bioconda
```

### Installing from cargo

cargo is the rust package manager. `simpleaf` is available on [crate.io](https://crates.io/crates/simpleaf) and can be installed from cargo.

```shell
cargo install simpleaf
```

### Building from source

You can also choose to build simpleaf from source by pulling its GitHub repo and build it as a normal rust program.

```shell
git clone https://github.com/DongzeHE/simpleaf.git && cd simpleaf
cargo build --release
```
