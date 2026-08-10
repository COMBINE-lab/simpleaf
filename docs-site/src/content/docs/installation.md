---
title: "Installation"
---

`Simpleaf` can be installed from source, from [crates.io](https://crates.io/crates/simpleaf), or installed via [bioconda](https://bioconda.github.io/recipes/simpleaf/README.html). `simpleaf` requires [alevin-fry](https://github.com/COMBINE-lab/alevin-fry), [piscem](https://github.com/COMBINE-lab/piscem), and `wget`.

:::caution
This release requires **piscem >= 0.22.0** and **alevin-fry >= 0.17.0**. These are hard requirements, not recommendations: `simpleaf set-paths` will reject older binaries. `piscem` 0.22.0 changed the meaning of `-t` (see [Threads and decompression](/simpleaf/threads-and-decompression/)) and added the build options `simpleaf` now passes unconditionally, so an older `piscem` cannot run these commands. If you install from bioconda, check that the versions it resolves meet these floors.
:::

## Recommended: installing from conda

We recommend all x86 (Linux or Mac) users to install `simpleaf` from bioconda, because all its dependencies are also available on conda, and will be automatically installed (except `piscem`) when installing `simpleaf`.

```sh
conda install simpleaf piscem -c bioconda -c conda-forge
```
**For Apple-silicon computers**, for example those with an Apple M-series chip, simpleaf should be installed under the x86 emulation layer, in other words, in shell with Rosetta2 enabled. See [this](https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=Attention%20Apple%20silicon%20computer%20users%3A) for details. Furthermore, if one would like to use `piscem` on apple silicon, one has to either download the [pre-built piscem executable](https://github.com/COMBINE-lab/piscem/releases) or build piscem from source **in the native shell (with Rosetta2 disabled)** using the commands described [here](https://github.com/COMBINE-lab/piscem#building). Then, piscem can be executed from both Rosetta2 enabled and disabled shell.

## Installing with cargo

cargo is the rust package manager. `simpleaf` is available on [crate.io](https://crates.io/crates/simpleaf) and can be installed from cargo.

```sh
cargo install simpleaf
```
Once installed, one will need to set the path to the executable of dependencies using the `simpleaf set-paths` program as discussed in section [Set Up Simpleaf manually](https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=4.%20Set%20Up%20Simpleaf%20Manually).

## Building from source (from GitHub)

You can also choose to build simpleaf from source by pulling its GitHub repo and build it as a normal rust program. Then, one needs to [set up simpleaf manually](https://combine-lab.github.io/alevin-fry-tutorials/2023/simpleaf-piscem/#:~:text=4.%20Set%20Up%20Simpleaf%20Manually).

```sh
git clone https://github.com/COMBINE-lab/simpleaf.git && cd simpleaf
cargo build --release
```
