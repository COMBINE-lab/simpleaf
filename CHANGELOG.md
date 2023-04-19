# Changelog

## [0.13.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.12.0...v0.13.0) (2023-04-17)


### Features

* simpleaf workflow refresh + display version in simpleaf workflow list ([c07bf99](https://github.com/COMBINE-lab/simpleaf/commit/c07bf993881d7977fcfa4ecab36c85e0dead6994))


### Bug Fixes

* change force update protocols estuary to an enum ([b2a7e8f](https://github.com/COMBINE-lab/simpleaf/commit/b2a7e8fd2afae7dbb44f34ad43e00b7a32f0d735))
* improve workflow list and add --ext-codes flag to workflow run ([a389e7f](https://github.com/COMBINE-lab/simpleaf/commit/a389e7f386995731b8321a1511c6ed2cadbbc1ac))

## [0.12.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.11.1...v0.12.0) (2023-04-10)


### Features

* improve autodetection of index type ([ed70433](https://github.com/COMBINE-lab/simpleaf/commit/ed704336051ccb07deff706e22d5ad6604618037))
* improve logging when parsing commands from workflow description ([083e714](https://github.com/COMBINE-lab/simpleaf/commit/083e714c53bf21f7d987943b2a20283aa41c88c5))
* improve logging when parsing commands from workflow description ([0d91772](https://github.com/COMBINE-lab/simpleaf/commit/0d91772236f7c94789f0fcedb079508156329201))

## [0.11.1](https://github.com/COMBINE-lab/simpleaf/compare/v0.11.0...v0.11.1) (2023-04-07)


### Bug Fixes

* add submodule to cargo.toml ([6475ced](https://github.com/COMBINE-lab/simpleaf/commit/6475cedd1009852de4b195bba88a1b2e8208d82c))

## [0.11.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.10.0...v0.11.0) (2023-04-07)


### Features

* add --skip-steps, --start-at, --resume for simpleaf workflow, and add get-workflow-config program ([be205b1](https://github.com/COMBINE-lab/simpleaf/commit/be205b1da3871651c8a13082077ace006736aa8e))
* Add Active field to workflow ([22e0096](https://github.com/COMBINE-lab/simpleaf/commit/22e009680da3de0de2a315ea56a53b904e34df3a))
* add more arguments and improve external command execution ([475aabf](https://github.com/COMBINE-lab/simpleaf/commit/475aabf102b1aee5433c282b3cb2603f669482e3))
* allow streaming transformation of complex geometry ([11e177b](https://github.com/COMBINE-lab/simpleaf/commit/11e177bbc92f448fc7df769555dab51de56c1347))
* bump seq_geom version and print xform stats ([4055cb5](https://github.com/COMBINE-lab/simpleaf/commit/4055cb5c7966542ff026969ac3c9e528ddefafad))
* improve workflow doc ([dd8f3c5](https://github.com/COMBINE-lab/simpleaf/commit/dd8f3c591da03b8762b52b6b507d64e8c49f989e))
* improve workflow logging ([07f811a](https://github.com/COMBINE-lab/simpleaf/commit/07f811a4c5ab2fcdcf1fa06057a89b8dcd2f77d9))
* simpleaf RunWorkflow ([21dd9d8](https://github.com/COMBINE-lab/simpleaf/commit/21dd9d84658061931ad449cf5947121035d0426e))


### Bug Fixes

* add explicit_pl to allowed filter arguments ([30f86e9](https://github.com/COMBINE-lab/simpleaf/commit/30f86e92eeb1e5775a70e2573eb22e4f6c45651a))
* fix incorrect collate cmd log recorded in the simpleaf_quant_log.json ([8f9a59f](https://github.com/COMBINE-lab/simpleaf/commit/8f9a59faaeb23727857b93f875792e9e1a4497f9))
* toy reference dir in testing dataset ([6a9ed0e](https://github.com/COMBINE-lab/simpleaf/commit/6a9ed0e5a849dbb4489f72a2789bdaa842cea979))
* toy reference dir in testing dataset ([095efcb](https://github.com/COMBINE-lab/simpleaf/commit/095efcb82692fab7a6b25a63af0b839b6492b888))
* Update af_utils.rs ([fff3b5e](https://github.com/COMBINE-lab/simpleaf/commit/fff3b5e8287c8fc9ad361747b5e6e6b028bfd212))
* update library call to get simplified description string ([4d87286](https://github.com/COMBINE-lab/simpleaf/commit/4d87286ce304a87a382553c21db77771ef245168))

## [0.10.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.9.0...v0.10.0) (2023-02-12)


### Features

* improve logging ([5d20450](https://github.com/COMBINE-lab/simpleaf/commit/5d204509e03211613ef74282b6d19a9ba53abd12))


### Bug Fixes

* add --overwrite flag to index command ([2340945](https://github.com/COMBINE-lab/simpleaf/commit/2340945ff585ea4b656a683cfba9c47532944f1b))

## [0.9.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.8.1...v0.9.0) (2023-02-08)


### Features

* attempt to infer t2g if not provided explicitly ([656a84a](https://github.com/COMBINE-lab/simpleaf/commit/656a84aab1731a1b40cfb2212c48a243e9bd4947))


### Bug Fixes

* be more informative on command error ([44fb9cb](https://github.com/COMBINE-lab/simpleaf/commit/44fb9cb5069254befe301af43f88495eaf26add5))
* fix duplicated short argument ([64a3010](https://github.com/COMBINE-lab/simpleaf/commit/64a3010f7f59c0c218000b3903551e8b99214e11))

## [0.8.1](https://github.com/COMBINE-lab/simpleaf/compare/v0.8.0...v0.8.1) (2023-01-03)


### Bug Fixes

* minor dep version bumps ([e3d79fe](https://github.com/COMBINE-lab/simpleaf/commit/e3d79feee4ac51e44c95c12afb6543548edaac14))

## [0.8.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.7.0...v0.8.0) (2023-01-03)


### Features

* switch to tracing for logging ([01538c6](https://github.com/COMBINE-lab/simpleaf/commit/01538c623e637387290edd0abf59d7803dfee1c2))


### Bug Fixes

* improve helping message ([9a3d851](https://github.com/COMBINE-lab/simpleaf/commit/9a3d85125edeecd6cf24b51997e79089b3fdc9c8))

## [0.7.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.6.0...v0.7.0) (2022-11-15)


### Features

* quant from map and min-reads ([0621a2d](https://github.com/COMBINE-lab/simpleaf/commit/0621a2d9e4ec00a39fe02a8e4e7ef0cf854a1661))

## [0.6.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.5.3...v0.6.0) (2022-10-26)


### Features

* create AF_HOME if needed in set-paths ([0d5411b](https://github.com/COMBINE-lab/simpleaf/commit/0d5411be55c7b16bbec76acee3eef93a2640d8d2))
* create AF_HOME if needed in set-paths ([742c2e2](https://github.com/COMBINE-lab/simpleaf/commit/742c2e2ea2d9caad79178c25a64c83ac27fa664e))
* create home dir if it doesn't exist ([bae0d42](https://github.com/COMBINE-lab/simpleaf/commit/bae0d42ca358b66eb827f2174f9238bbcb8ad5f2))

## [0.5.3](https://github.com/COMBINE-lab/simpleaf/compare/v0.5.2...v0.5.3) (2022-10-11)


### Bug Fixes

* expect cells ([c163249](https://github.com/COMBINE-lab/simpleaf/commit/c163249fb6751aa4f445bd99e9a3c4df9a0da476))

## [0.5.2](https://github.com/COMBINE-lab/simpleaf/compare/v0.5.1...v0.5.2) (2022-10-01)


### Bug Fixes

* add version flag ([786df29](https://github.com/COMBINE-lab/simpleaf/commit/786df297d228af758a9291ef7c6166252ddfccc3))

## [0.5.1](https://github.com/COMBINE-lab/simpleaf/compare/v0.5.0...v0.5.1) (2022-08-23)


### Bug Fixes

* parsing of multiple files for quant ([349642e](https://github.com/COMBINE-lab/simpleaf/commit/349642e05f537ed18243a3599bc770c20ca6f325))

## [0.5.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.4.0...v0.5.0) (2022-08-21)


### Features

* added ability to set expected-ori in `quant` ([66aab95](https://github.com/COMBINE-lab/simpleaf/commit/66aab95ed7a66b9a8cfe2349d007662eb944b978))

## [0.4.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.3.0...v0.4.0) (2022-08-04)


### Features

* allow index to take targets directly ([20750a6](https://github.com/COMBINE-lab/simpleaf/commit/20750a6505e802382f2922b5832ee68c25000933))
* can pass -u flag to quant with file ([f795940](https://github.com/COMBINE-lab/simpleaf/commit/f7959408cfd65dfa7fee5fc0fd7621d206f2cbe8))

## [0.3.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.2.1...v0.3.0) (2022-08-02)


### Features

* add chemistries command ([75e3dbe](https://github.com/COMBINE-lab/simpleaf/commit/75e3dbeb5e9caeab2d7f5fa2e3311cc1dfb744ae))


### Bug Fixes

* doc and usage ([a52f866](https://github.com/COMBINE-lab/simpleaf/commit/a52f866c337a21bd75f870c65fca2a5026f18ef5))

## [0.2.1](https://github.com/COMBINE-lab/simpleaf/compare/v0.2.0...v0.2.1) (2022-07-29)


### Bug Fixes

* fix Cargo.toml ([179dfdc](https://github.com/COMBINE-lab/simpleaf/commit/179dfdc945ff707fd538e35028817493dcecb9ba))

## [0.2.0](https://github.com/COMBINE-lab/simpleaf/compare/v0.1.1...v0.2.0) (2022-07-29)


### ⚠ BREAKING CHANGES

* add required cargo fields

### Bug Fixes

* add required cargo fields ([becc3be](https://github.com/COMBINE-lab/simpleaf/commit/becc3be55254b75053f7d31252883efe03644092))
* make tags match ([a91f6fb](https://github.com/COMBINE-lab/simpleaf/commit/a91f6fb3d9d994f80e04bbb86b08bb01a11b6c4f))

## [0.1.1](https://github.com/COMBINE-lab/simpleaf/compare/v0.1.0...v0.1.1) (2022-07-29)


### Bug Fixes

* try to force cargo publish ([1f13cb5](https://github.com/COMBINE-lab/simpleaf/commit/1f13cb51d197556b5ccd808becbe1cb4aff67596))

## 0.1.0 (2022-07-29)


### Bug Fixes

* change ci branch to main ([3229283](https://github.com/COMBINE-lab/simpleaf/commit/322928375defb6a6124d277ce9aeabda1d94dd2f))
* Simply match statements to if let ([3c25d20](https://github.com/COMBINE-lab/simpleaf/commit/3c25d201df4d5a7b5532f3309c3e3740aa8cb7c6))
