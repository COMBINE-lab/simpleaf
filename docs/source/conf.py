# Configuration file for the Sphinx documentation builder.
#
# This file only contains a selection of the most common options. For a full
# list see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Path setup --------------------------------------------------------------

# If extensions (or modules to document with autodoc) are in another directory,
# add these directories to sys.path here. If the directory is relative to the
# documentation root, use os.path.abspath to make it absolute, like shown here.
#
# import os
# import sys
# sys.path.insert(0, os.path.abspath('.'))


# -- Project information -----------------------------------------------------

project = 'simpleaf'
copyright = '2022-, Dongze He, Noor Pratap Singh, Rob Patro'
author = 'Dongze He, Noor Pratap Singh, Rob Patro'

# The full version, including alpha/beta/rc tags
release = '0.27.0'

# These docs are frozen. Development continues at the Astro/Starlight site, and
# rst_prolog is the one hook that reaches every page without touching each file.
rst_prolog = """
.. attention::

   **These docs have moved.** The current simpleaf documentation lives at
   https://combine-lab.github.io/simpleaf and is the only version that is
   updated. This readthedocs site is a frozen snapshot of the 0.27.0 docs and
   will not track later releases.
"""


master_doc = 'index'

# -- General configuration ---------------------------------------------------

# Add any Sphinx extension module names here, as strings. They can be
# extensions coming with Sphinx (named 'sphinx.ext.*') or your custom
# ones.
extensions = ['sphinx.ext.autosectionlabel']

# Add any paths that contain templates here, relative to this directory.
templates_path = ['_templates']

# List of patterns, relative to source directory, that match files and
# directories to ignore when looking for source files.
# This pattern also affects html_static_path and html_extra_path.
exclude_patterns = []


# -- Options for HTML output -------------------------------------------------

# The theme to use for HTML and HTML Help pages.  See the documentation for
# a list of builtin themes.
#
html_theme = 'furo'

html_logo = '../logo.png'

pygments_style = "sphinx"
pygments_dark_style = "monokai"

# Add any paths that contain custom static files (such as style sheets) here,
# relative to this directory. They are copied after the builtin static files,
# so a file named "default.css" will overwrite the builtin "default.css".
# html_static_path = ['_static']
