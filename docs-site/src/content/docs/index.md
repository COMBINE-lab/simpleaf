---
title: "Welcome to the documentation for simpleaf"
---

## What is simpleaf?

`simpleaf` is a program to simplify and customize the running and 
configuration of single-cell processing with [alevin-fry](https://github.com/COMBINE-lab/alevin-fry/).
This documentation covers the main commands of `simpleaf`, and how the program works. 
The tutorials we have made for simpleaf are available at [alevin-fry tutorial website](https://combine-lab.github.io/alevin-fry-tutorials/).

## Important note

The `simpleaf` program runs tools uses in the `alevin-fry` pipeline.  Specifically, 
to make use of the core functionality of this tool, you will need to install 
[piscem](https://github.com/COMBINE-lab/piscem/) and
[alevin-fry](https://github.com/COMBINE-lab/alevin-fry/). Further, in order to operate properly, 
`simpleaf` **requires that you set the environment variable** `ALEVIN_FRY_HOME`. It will use the directory 
pointed to by this variable to cache useful information (e.g. the paths to selected versions of 
the tools mentioned above, the mappings for custom chemistries you tell it about, and other information 
like the permit lsits for certain chemistries).  So, before you run `simpleaf`, please make sure that you 
set the `ALEVIN_FRY_HOME` environment variable (you can also set it on the command line when you run 
`simpleaf`, but setting it in your environment once is much simpler).  In most shells, this can be done with

```sh
$ export ALEVIN_FRY_HOME=/full/path/to/dir/you/want/to/use
```
That's it for initial notes.  Use the sidebar to learn more about the `simpleaf` commands.
