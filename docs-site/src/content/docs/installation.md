---
title: "Installation"
---

`Simpleaf` can be installed from source, from [crates.io](https://crates.io/crates/simpleaf), or installed via [bioconda](https://bioconda.github.io/recipes/simpleaf/README.html). `simpleaf` requires [alevin-fry](https://github.com/COMBINE-lab/alevin-fry), [piscem](https://github.com/COMBINE-lab/piscem), and `wget`.

:::caution
This release requires **piscem >= 0.22.0** and **alevin-fry >= 0.18.0**. These are hard requirements, not recommendations: `simpleaf set-paths` will reject older binaries. `piscem` 0.22.0 changed the meaning of `-t` (see [Threads and decompression](/simpleaf/threads-and-decompression/)), while alevin-fry 0.18.0 provides the deterministic correction and resource controls forwarded by simpleaf 0.28. If you install from bioconda, check that the versions it resolves meet these floors.
:::

## Recommended: installing from conda

We recommend that Linux and macOS users install `simpleaf` from bioconda, because all its dependencies are also available on conda, and will be automatically installed (except `piscem`) when installing `simpleaf`.

```sh
conda install simpleaf piscem -c bioconda -c conda-forge
```
**Apple-silicon and ARM users** need no special handling. bioconda ships native `osx-arm64` and `linux-aarch64` packages for `simpleaf`, `piscem`, and `alevin-fry`, so the command above works as-is in a native shell. Rosetta2 is not required, and installing under emulation is no longer recommended.

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
