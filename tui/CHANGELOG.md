# Changelog

## [3.6.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.5.0...lp_diff-v3.6.0) (2026-07-09)


### Documentation

* tighten demo captions in both READMEs ([d2dd437](https://github.com/dandxy89/lp_parser_rs/commit/d2dd4376e7c82cdff8c3f3a80ad9014f77cfb428))
* **tui:** document single-file inspect mode ([72953ed](https://github.com/dandxy89/lp_parser_rs/commit/72953ed89ae6c21f9cffc4f0553ccc847a9573c4))
* **tui:** redo demo GIF as a full feature tour of an MPS vs LP diff ([9ef549f](https://github.com/dandxy89/lp_parser_rs/commit/9ef549f15b89f0a4b3a9d98511dba86755e60856))


### Features

* **core:** extract LP diff engine into library behind diff feature ([bb6a7a5](https://github.com/dandxy89/lp_parser_rs/commit/bb6a7a57311ae6c31bec92ffd351cb48b0f7bdc5))
* **tui:** add single-file inspect mode selection and model ([9473c76](https://github.com/dandxy89/lp_parser_rs/commit/9473c76b0d825ab0cacf750154df5c52658af911))
* **tui:** bracketed paste, kitty keyboard protocol, and clean error restore ([66d9622](https://github.com/dandxy89/lp_parser_rs/commit/66d962293e04c583aa8ffdfdfe715642dbc5702d))
* **tui:** breadcrumb detail-panel titles with raw-view toggle hint ([85b0f7d](https://github.com/dandxy89/lp_parser_rs/commit/85b0f7d3ae4d2371a6c64e9a1cb3958f36d1c2f8))
* **tui:** c: prefix for full-text content search ([44579a6](https://github.com/dandxy89/lp_parser_rs/commit/44579a6c0eb60796ebb0f862ce11196b8b36816b))
* **tui:** context-sensitive status-bar hints and yank which-key ([58378d3](https://github.com/dandxy89/lp_parser_rs/commit/58378d3c8cbcdc9bdca6f6d0fb81950e58aa9963))
* **tui:** delta column and sort indicator under the delta sorts ([5e08098](https://github.com/dandxy89/lp_parser_rs/commit/5e080986f1ff22e7198697e7692c6b2044ea56b0))
* **tui:** empty-detail cheat sheet and palette gaps filled ([7729fd0](https://github.com/dandxy89/lp_parser_rs/commit/7729fd0677a310efd59ecfd85a3835687a906a5b))
* **tui:** n/N to jump between matches of the last search ([04a06a2](https://github.com/dandxy89/lp_parser_rs/commit/04a06a2f124426a2c39ab47a0071960700b2b3c7))
* **tui:** per-kind change counts and active filter in the tab bar ([09a5437](https://github.com/dandxy89/lp_parser_rs/commit/09a54373be0ee05694ef23484af7edc5762fbafd))
* **tui:** readline-style editing for query and what-if inputs ([7ebd5b3](https://github.com/dandxy89/lp_parser_rs/commit/7ebd5b300d2c7c932921806abb66fe95bdeea642))
* **tui:** render inspect mode neutrally and gate diff-only actions ([6a2c98d](https://github.com/dandxy89/lp_parser_rs/commit/6a2c98df49a72b07962a19b135caa34a0e4f69fb))
* **tui:** report conflicting variable bounds in infeasibility diagnosis ([a2698dc](https://github.com/dandxy89/lp_parser_rs/commit/a2698dc1807d7179203b49a26f2fbcd7533d584b))
* **tui:** scrollbar on the detail panel ([f6ba364](https://github.com/dandxy89/lp_parser_rs/commit/f6ba3642cedf6d121ad90f9911c59587680b4761))
* **tui:** what-if constraint RHS editing with baseline re-solve ([6cc0b74](https://github.com/dandxy89/lp_parser_rs/commit/6cc0b746d0b0a08f2d944be4d6a4e5cb4e52069c))


### Bug Fixes

* parser, MPS, writer, and diff edge-case bugs found by test audit ([5d5a073](https://github.com/dandxy89/lp_parser_rs/commit/5d5a0730e901367db62ff1b03f4bd21b39d7b45b))
* **python:** declare dual MIT OR Apache-2.0 licence on PyPI ([de7d14e](https://github.com/dandxy89/lp_parser_rs/commit/de7d14e503f01ceb1b52259283f4049d46a9db42))
* **tui:** clamp detail-panel scroll to content height ([9a2bd67](https://github.com/dandxy89/lp_parser_rs/commit/9a2bd6707c8d9a2c02b2b281e2a9b9171f11c856))
* **tui:** give each solve its own solver-log temp file ([c3becc9](https://github.com/dandxy89/lp_parser_rs/commit/c3becc9e6de215f77bdb1eb2d7fb715aa0b8760f))
* **tui:** neutralise search pop-up in inspect mode ([b6c7ae9](https://github.com/dandxy89/lp_parser_rs/commit/b6c7ae9a23fdb7e8df365e19d984d0a8c8910ea7))
* **tui:** use as_chunks for rename-rule pairs to satisfy clippy ([e37a5ab](https://github.com/dandxy89/lp_parser_rs/commit/e37a5ab508791f12c4d9f3054a04463ae6783aaf))


### Performance Improvements

* **tui:** order coefficient diff by NameId and window the solve overlay ([552670b](https://github.com/dandxy89/lp_parser_rs/commit/552670b698f9d523c3fb40ea9f34dd5a657851d8))


### Code Refactoring

* remove over-engineering flagged by repo audit ([5d3ec69](https://github.com/dandxy89/lp_parser_rs/commit/5d3ec6951243ec27362992240bc839e10753892c))
* strip over-engineering found by repo-wide audit ([c842456](https://github.com/dandxy89/lp_parser_rs/commit/c842456c5fdd1d67ed16fea3dd17d9de71d6393a))
* **tui:** dedupe jumplist handlers and centralise filter mutation ([c29978d](https://github.com/dandxy89/lp_parser_rs/commit/c29978d2e44a0be940de8b952dd2d80a74832155))
* **tui:** dedupe shared helpers and avoid rename-rewrite allocation ([2afc5b2](https://github.com/dandxy89/lp_parser_rs/commit/2afc5b2f22a8238cce9c3d612cd0ae99a8ac5d42))


### Tests

* **tui:** UI snapshot tests for the main layouts ([da9000d](https://github.com/dandxy89/lp_parser_rs/commit/da9000d48947e7619fe9425240a55d77cc13c168))

## [3.5.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.4.0...lp_diff-v3.5.0) (2026-07-03)


### Build System

* Updating the TUI ([fb6c312](https://github.com/dandxy89/lp_parser_rs/commit/fb6c3126ecbf2430f753e6fca7938d33b4a56c3d))


### Miscellaneous Chores

* enable clippy pedantic workspace-wide and fix all violations ([b334191](https://github.com/dandxy89/lp_parser_rs/commit/b3341919d350ec4ba6d8ef5eb463c8a239a1a0d2))


### Documentation

* fix stale examples and links, fill missing docs, enforce doc lints ([e0bb357](https://github.com/dandxy89/lp_parser_rs/commit/e0bb3577dc3a5c3c5d893b46b3931caad4a45c72))
* refresh READMEs and tag the summary output code fence ([d718289](https://github.com/dandxy89/lp_parser_rs/commit/d7182890167a342508db6d1daaf7d18e9d89d94e))
* **tui:** add VHS demo GIF to READMEs ([9741da5](https://github.com/dandxy89/lp_parser_rs/commit/9741da593a48c01dc0a064ae9a955f647a4a2dcc))


### Features

* tui watch numerics perf ([#193](https://github.com/dandxy89/lp_parser_rs/issues/193)) ([d849d2f](https://github.com/dandxy89/lp_parser_rs/commit/d849d2f6bfa597cdaa59f421f79aa7f9ec571eeb))
* **tui:** cycle sections with [ and ], focus content on switch ([2bf86c8](https://github.com/dandxy89/lp_parser_rs/commit/2bf86c8309b2261659f6efd998097de8136111d2))
* **tui:** honour NO_COLOR and handle Ctrl+Z suspend ([e43baf3](https://github.com/dandxy89/lp_parser_rs/commit/e43baf35855e0c28c73cd60d4ae45a491fe6ecab))
* **tui:** scrollable help, command palette, and min-size guard ([3cb7d8b](https://github.com/dandxy89/lp_parser_rs/commit/3cb7d8bef1522c90a58c6dae0058c9abe9bced7c))


### Performance Improvements

* **tui:** gate idle-tick redraws, throttle watch polls, shrink helpers ([97854b4](https://github.com/dandxy89/lp_parser_rs/commit/97854b4c77e9b7470b2d159eb10cc32068ede309))


### Code Refactoring

* drop unused manual Debug for CompiledSearch ([ab050c9](https://github.com/dandxy89/lp_parser_rs/commit/ab050c90205dbef090da64e2d2713ea4682d13ee))
* ponytail audit cleanup ([#195](https://github.com/dandxy89/lp_parser_rs/issues/195)) ([ada04ce](https://github.com/dandxy89/lp_parser_rs/commit/ada04ceebb112725ce75c7fade7bba59c73ab0d8))
* **tui:** drop tempfile dependency, dedupe formatters ([8a5faf0](https://github.com/dandxy89/lp_parser_rs/commit/8a5faf00e06c8382d026f69a448c739704397cfb))
* use chrono for the reload-flash clock ([2b325e1](https://github.com/dandxy89/lp_parser_rs/commit/2b325e1789c12b0eb78c45fea49c94decdf808ae))


### Tests

* gate serde-only analysis test imports ([22ef729](https://github.com/dandxy89/lp_parser_rs/commit/22ef7290f657a6bf3c7b08756615d1bf87dff67b))

## [3.4.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.3.1...lp_diff-v3.4.0) (2026-06-10)


### Features

* TUI and Perf fixes ([#189](https://github.com/dandxy89/lp_parser_rs/issues/189)) ([c898123](https://github.com/dandxy89/lp_parser_rs/commit/c898123543df4000ab3ce3aca9adcaedb2059a6f))
* **tui:** add --abs-tol, --rel-tol, --rename to lp_diff ([#186](https://github.com/dandxy89/lp_parser_rs/issues/186)) ([adfce3a](https://github.com/dandxy89/lp_parser_rs/commit/adfce3a7052e267890c2b18c5086d1a30e9f6e75))

## [3.3.1](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.3.0...lp_diff-v3.3.1) (2026-04-20)


### Build System

* Updating crates in TUI ([a94568c](https://github.com/dandxy89/lp_parser_rs/commit/a94568cffb2e449ebf0e2f1824f0098f0e5d75f0))


### Miscellaneous Chores

* **deps:** update frizbee requirement in the cargo-dependencies group ([#182](https://github.com/dandxy89/lp_parser_rs/issues/182)) ([05e3985](https://github.com/dandxy89/lp_parser_rs/commit/05e3985a95cb8d072a8de6ae2ec866027b97a9ea))

## [3.3.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.2.0...lp_diff-v3.3.0) (2026-03-16)


### Features

* Adding support for MPS files ([#176](https://github.com/dandxy89/lp_parser_rs/issues/176)) ([a615c54](https://github.com/dandxy89/lp_parser_rs/commit/a615c543616da1c344c915b9ee878fc2c95532b9))
* Adding toggle to hide ordering differences in Detail window ([#179](https://github.com/dandxy89/lp_parser_rs/issues/179)) ([ef8ec83](https://github.com/dandxy89/lp_parser_rs/commit/ef8ec8371776a7f23d859160e758298bde27ea5d))
* Extending TUI to also support .mps files ([#178](https://github.com/dandxy89/lp_parser_rs/issues/178)) ([934cde8](https://github.com/dandxy89/lp_parser_rs/commit/934cde8a1b1c954239bd3d2bc190119a3e145c91))
* Included MPS diff capability into the TUI ([#177](https://github.com/dandxy89/lp_parser_rs/issues/177)) ([a4d15b3](https://github.com/dandxy89/lp_parser_rs/commit/a4d15b39acde3d2d9ec6a4a1d3db7973ac51ab3c))


### Styles

* Clippy lints ([770813b](https://github.com/dandxy89/lp_parser_rs/commit/770813b9b4ac62f795f79535945e5e0861b60fce))

## [3.2.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.1.1...lp_diff-v3.2.0) (2026-03-02)


### Documentation

* **tui:** Update README with new features ([a838e75](https://github.com/dandxy89/lp_parser_rs/commit/a838e75b402470b64bce373bc3441e2ff25b893b))


### Features

* **tui:** Add CSV export of diff report ([fa53f7f](https://github.com/dandxy89/lp_parser_rs/commit/fa53f7fb8108b81cca26f8b0adf25324b5579ea3))
* **tui:** Add raw text side-by-side diff toggle ([24206e6](https://github.com/dandxy89/lp_parser_rs/commit/24206e6fdc7bd4d8bac79eb93d4412f27791d298))
* **tui:** Add summary yank support ([7180b07](https://github.com/dandxy89/lp_parser_rs/commit/7180b07e125747aa2cdae6ddea47da714cfdca9c))
* **tui:** Add yo/yn chords to yank old/new side ([5e7060a](https://github.com/dandxy89/lp_parser_rs/commit/5e7060a557becc2f45f8142072663f58f3cac644))


### Bug Fixes

* Scrolling logic ([#170](https://github.com/dandxy89/lp_parser_rs/issues/170)) ([7e4588d](https://github.com/dandxy89/lp_parser_rs/commit/7e4588d2d88b0aeb69216d51b412ee0e11c897f3))

## [3.1.1](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.1.0...lp_diff-v3.1.1) (2026-02-24)


### Bug Fixes

* Show solver timings ([0ac44ea](https://github.com/dandxy89/lp_parser_rs/commit/0ac44eac7b606040d3ac07fe4327ee52d69b4dd2))


### Code Refactoring

* Make tui theme const ([b58821b](https://github.com/dandxy89/lp_parser_rs/commit/b58821b8bfa8179604e5af9e4fabcff5e9496846))

## [3.1.0](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.0.2...lp_diff-v3.1.0) (2026-02-24)


### Build System

* Adding release profile ([526ddec](https://github.com/dandxy89/lp_parser_rs/commit/526ddecfa9702cf055daffc2d0771f011cc7d304))


### Continuous Integration

* fix release-please config paths and changelog locations ([5808e72](https://github.com/dandxy89/lp_parser_rs/commit/5808e72bceb4aefd1c0cbe69a4902ebf42dcabc3))
* trigger release-please after tag fix ([bb19a1c](https://github.com/dandxy89/lp_parser_rs/commit/bb19a1cdbac44a957ff8ce07ebbe5e5f729eb2c1))


### Miscellaneous Chores

* release main ([#164](https://github.com/dandxy89/lp_parser_rs/issues/164)) ([8a8e267](https://github.com/dandxy89/lp_parser_rs/commit/8a8e2671887a8983ed009f0862fbe56784e1569f))


### Features

* Delta toggle ([b5497fd](https://github.com/dandxy89/lp_parser_rs/commit/b5497fdb926a9e4789cc38ca5c168c84d2b84c8a))


### Bug Fixes

* Cache solve overlay formatted lines ([d41603e](https://github.com/dandxy89/lp_parser_rs/commit/d41603e4bce98ff240a38c0d3f727f20c008aa25))
* Early Exit for Unchanged Constraint Pairs ([3f0e148](https://github.com/dandxy89/lp_parser_rs/commit/3f0e14884ae77c104d66f99f3edb0015b2fb1e48))
* Eliminate String Allocation in Sort Functions ([0f89182](https://github.com/dandxy89/lp_parser_rs/commit/0f891823bb028f10ab65901a21b0b1a6115181f5))
* lazy name resolution in diff ([c97de70](https://github.com/dandxy89/lp_parser_rs/commit/c97de70412278a064092c26879cd4c93e0677897))
* memmap2 = { version = "0.9.10", optional = true } ([e11767f](https://github.com/dandxy89/lp_parser_rs/commit/e11767ff5fbf893528eb85c2fd1b1bdc55b501ed))
* Pass Parsed Problems to Solver ([a9f8062](https://github.com/dandxy89/lp_parser_rs/commit/a9f80621e99c9e297051c7b0c32cc0eff89b5037))
* Replace BTreeMap with HashMap in solver.rs ([130a35d](https://github.com/dandxy89/lp_parser_rs/commit/130a35dc321e895ad3471db0608dabee6c0b22b3))
* solver use NameId keys instead of owned Strings ([09736f9](https://github.com/dandxy89/lp_parser_rs/commit/09736f983c98c0ddedc16f5dcf81fdd8f8a1257d))
* Update LineMap to use NameId ([94cb373](https://github.com/dandxy89/lp_parser_rs/commit/94cb37384e4c13936a070a5648a9012d621d1b8e))


### Performance Improvements

* eliminate writer allocations, derive Copy for small enums, simplify variable collection, refactor analysis functions ([cd4f331](https://github.com/dandxy89/lp_parser_rs/commit/cd4f331588e3bacc86285d0ceb9993297c8a1dc2))
* TUI improvements ([759ab98](https://github.com/dandxy89/lp_parser_rs/commit/759ab98448668b61850d7ff2269a621223a78782))


### Styles

* Remove inline imports and qualified imports ([42c20a2](https://github.com/dandxy89/lp_parser_rs/commit/42c20a25b412b6fded358fdca70db4983db6f19b))

## [3.0.2](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v0.1.0...lp_diff-v3.0.2) (2026-02-23)


### Build System

* Adding release profile ([526ddec](https://github.com/dandxy89/lp_parser_rs/commit/526ddecfa9702cf055daffc2d0771f011cc7d304))


### Miscellaneous Chores

* Override release version ([68a74fc](https://github.com/dandxy89/lp_parser_rs/commit/68a74fc2e8709550571aacd945fd06be2695719c))
* release 1.3.0 ([3af4fcf](https://github.com/dandxy89/lp_parser_rs/commit/3af4fcf18a388140faf324bd5f2459aef65d9f75))
* release 1.3.1 ([38f57f6](https://github.com/dandxy89/lp_parser_rs/commit/38f57f6b6aa98b2c1012a903f628718245959ffc))
* release 1.4.0 ([cce8326](https://github.com/dandxy89/lp_parser_rs/commit/cce8326881b55e8b070f58665812e2b3e40e1624))
* release 2.0.0 ([666d968](https://github.com/dandxy89/lp_parser_rs/commit/666d968d1c47d5e7eff2618f702ecb1da74a1295))
* release 2.3.0 ([efafbca](https://github.com/dandxy89/lp_parser_rs/commit/efafbcae561ea76e020156e5f3d9fc7c273e4156))
* release 2.4.0 ([63fd277](https://github.com/dandxy89/lp_parser_rs/commit/63fd2773fe2375b2e2f95b603287cdf1683934f6))
* release 3.0.0 ([91c5c59](https://github.com/dandxy89/lp_parser_rs/commit/91c5c592ed015cecbf573b900ddd3db2980c1aab))
* release 3.0.2 ([e6771bd](https://github.com/dandxy89/lp_parser_rs/commit/e6771bd29fda5fdb7fa4c424b7892b8fbd9678ea))


### Features

* Adding a dedicated TUI for solving, viewing and comparing LP files ([#160](https://github.com/dandxy89/lp_parser_rs/issues/160)) ([8b6c455](https://github.com/dandxy89/lp_parser_rs/commit/8b6c455133f4bc226796fd166d577d57e8b26871))
* Delta toggle ([b5497fd](https://github.com/dandxy89/lp_parser_rs/commit/b5497fdb926a9e4789cc38ca5c168c84d2b84c8a))


### Bug Fixes

* Cache solve overlay formatted lines ([d41603e](https://github.com/dandxy89/lp_parser_rs/commit/d41603e4bce98ff240a38c0d3f727f20c008aa25))
* Early Exit for Unchanged Constraint Pairs ([3f0e148](https://github.com/dandxy89/lp_parser_rs/commit/3f0e14884ae77c104d66f99f3edb0015b2fb1e48))
* Eliminate String Allocation in Sort Functions ([0f89182](https://github.com/dandxy89/lp_parser_rs/commit/0f891823bb028f10ab65901a21b0b1a6115181f5))
* lazy name resolution in diff ([c97de70](https://github.com/dandxy89/lp_parser_rs/commit/c97de70412278a064092c26879cd4c93e0677897))
* memmap2 = { version = "0.9.10", optional = true } ([e11767f](https://github.com/dandxy89/lp_parser_rs/commit/e11767ff5fbf893528eb85c2fd1b1bdc55b501ed))
* Pass Parsed Problems to Solver ([a9f8062](https://github.com/dandxy89/lp_parser_rs/commit/a9f80621e99c9e297051c7b0c32cc0eff89b5037))
* Replace BTreeMap with HashMap in solver.rs ([130a35d](https://github.com/dandxy89/lp_parser_rs/commit/130a35dc321e895ad3471db0608dabee6c0b22b3))
* solver use NameId keys instead of owned Strings ([09736f9](https://github.com/dandxy89/lp_parser_rs/commit/09736f983c98c0ddedc16f5dcf81fdd8f8a1257d))
* Update LineMap to use NameId ([94cb373](https://github.com/dandxy89/lp_parser_rs/commit/94cb37384e4c13936a070a5648a9012d621d1b8e))


### Performance Improvements

* eliminate writer allocations, derive Copy for small enums, simplify variable collection, refactor analysis functions ([cd4f331](https://github.com/dandxy89/lp_parser_rs/commit/cd4f331588e3bacc86285d0ceb9993297c8a1dc2))
* TUI improvements ([759ab98](https://github.com/dandxy89/lp_parser_rs/commit/759ab98448668b61850d7ff2269a621223a78782))


### Styles

* Remove inline imports and qualified imports ([42c20a2](https://github.com/dandxy89/lp_parser_rs/commit/42c20a25b412b6fded358fdca70db4983db6f19b))
