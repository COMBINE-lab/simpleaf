## Contributing code

Any code that you contribute will be licensed under the BSD 3-clause license adopted by simpleaf.

Code contributions should be made via pull requests.  Please make all PRs to the _dev_ branch 
of the repository.  PRs made to the _main_ branch may be rejected if they cannot be cleanly rebased 
on _dev_.  Before you make a PR, please check that:

 * commit messages should be made using [*conventional commits*](https://www.conventionalcommits.org/en/v1.0.0/) — please format all of your commit messages as such.
 * you've run `cargo fmt` on the relevant code.
 * any non-obvious code is documented (we don't yet have formal documentation guidelines, so use common sense)
 * you've run `cargo clippy` on the relevant code and any issues are either resolved or the PR describes why they were ignored.

## Cutting a release

`bump_and_publish.sh` is the sole supported release entry point. It validates
the requested semantic version and clean upstream state, checks the crate,
verifies its crates.io package when `--publish` is selected, updates both the
manifest and lockfile, commits, tags, pushes, and optionally publishes. Preview
the release without changing repository or registry state with:

```sh
./bump_and_publish.sh 0.28.0 --publish --dry-run
```

Do not create a release commit or tag with a separate version-bump script.
