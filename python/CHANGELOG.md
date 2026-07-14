# Changelog

## [3.0.2](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v4.0.1...parse_lp-v3.0.2) (2026-07-14)


### ⚠ BREAKING CHANGES

* **python:** parse() now raises LpParseError immediately for malformed files instead of deferring the error to the first attribute access.
* **python:** parse-lp console script and LpParser.compare() are removed; use the Rust CLI's diff subcommand instead.

### Build System

* **deps:** bump pygments from 2.19.2 to 2.20.0 in /python ([#180](https://github.com/dandxy89/lp_parser_rs/issues/180)) ([4e4bd0f](https://github.com/dandxy89/lp_parser_rs/commit/4e4bd0f568ce087b832dc727f015eff46fdf555a))
* Python updates + lint fixes ([e6693a0](https://github.com/dandxy89/lp_parser_rs/commit/e6693a03111ce494bc90196f93c176122c47cda2))
* Raise MSRV to 1.70.0 and remove once_lock ([f80a1bf](https://github.com/dandxy89/lp_parser_rs/commit/f80a1bf26a1c011b014b84f391799211d5f0e960))
* update pyo3 to version 0.28.0 and replace pyrefly with ty in dependencies ([38907e5](https://github.com/dandxy89/lp_parser_rs/commit/38907e513cb5865aca186c253d8c48dab48dc5ec))
* Updating crates, ty and ruff ([76bbebc](https://github.com/dandxy89/lp_parser_rs/commit/76bbebc7ef216386f5f856dbac718cbc4e60cdbd))
* Updating py03 ([f947950](https://github.com/dandxy89/lp_parser_rs/commit/f9479506b600c6b711b41e1e0cd0a661db7a3674))
* Updating py03 ([39b1c8a](https://github.com/dandxy89/lp_parser_rs/commit/39b1c8a67d61603944eaca5cf4296db3c29906d7))
* Updating pyproject ([c998e5d](https://github.com/dandxy89/lp_parser_rs/commit/c998e5ddd3ad3aa4bf88af90c506659ec5d01436))
* Updating Python project toml and makefile ([afdc2c5](https://github.com/dandxy89/lp_parser_rs/commit/afdc2c58b47c4f657a8724c9da02d5e93c548e33))
* Upgrading to use nom 0.8.0 ([#112](https://github.com/dandxy89/lp_parser_rs/issues/112)) ([4f8a0e3](https://github.com/dandxy89/lp_parser_rs/commit/4f8a0e326aaad54ad76eb88ebbb2775ea6740454))


### Continuous Integration

* Adding python build runner ([031414d](https://github.com/dandxy89/lp_parser_rs/commit/031414de5b3baaed478a35cde4a387439d6a4791))
* Fix pipeline sync ([cb8d0bd](https://github.com/dandxy89/lp_parser_rs/commit/cb8d0bda41be746647a7628ab3071f6332b67a4e))
* fix release-please config paths and changelog locations ([5808e72](https://github.com/dandxy89/lp_parser_rs/commit/5808e72bceb4aefd1c0cbe69a4902ebf42dcabc3))
* Retrying python deploy ([a6aac40](https://github.com/dandxy89/lp_parser_rs/commit/a6aac40e2ac246d84e6050042695a2a6c5c29c76))
* trigger release-please after tag fix ([bb19a1c](https://github.com/dandxy89/lp_parser_rs/commit/bb19a1cdbac44a957ff8ce07ebbe5e5f729eb2c1))
* Updating manifest ([b16dcbc](https://github.com/dandxy89/lp_parser_rs/commit/b16dcbcbbdd052eeae3887c9a58fe7edf3449164))
* Updating python package ([7757d7f](https://github.com/dandxy89/lp_parser_rs/commit/7757d7fcf6a205bff9d2737beb788a6cddf16726))
* Updating versions ([492517d](https://github.com/dandxy89/lp_parser_rs/commit/492517d9e257608d39aa70cf2bdd8b9e8ca98f5b))


### Miscellaneous Chores

* Adding more validation / debug_asserts ([cdc94bd](https://github.com/dandxy89/lp_parser_rs/commit/cdc94bd8d39c910faf27529e187bf9a07bf904b7))
* Checking the project runs with the latest version of Python ([d6cf0da](https://github.com/dandxy89/lp_parser_rs/commit/d6cf0da46ad76d4895c7a4ba5f83f1ae50ffea1d))
* **deps-dev:** bump basedpyright ([#183](https://github.com/dandxy89/lp_parser_rs/issues/183)) ([95cab0a](https://github.com/dandxy89/lp_parser_rs/commit/95cab0abdb164057fb51f6b2c8adec6fa8f7569c))
* **deps:** update pyo3 requirement ([#140](https://github.com/dandxy89/lp_parser_rs/issues/140)) ([742129b](https://github.com/dandxy89/lp_parser_rs/commit/742129b071a2ee7f4fdf4e8508ad5841ed8bb7da))
* **deps:** update pyo3 requirement in the cargo-dependencies group ([#139](https://github.com/dandxy89/lp_parser_rs/issues/139)) ([16e732f](https://github.com/dandxy89/lp_parser_rs/commit/16e732f460eeeb1acd9d0564c75c862218a4d5a7))
* enable clippy pedantic workspace-wide and fix all violations ([b334191](https://github.com/dandxy89/lp_parser_rs/commit/b3341919d350ec4ba6d8ef5eb463c8a239a1a0d2))
* Override release version ([68a74fc](https://github.com/dandxy89/lp_parser_rs/commit/68a74fc2e8709550571aacd945fd06be2695719c))
* release 1.3.0 ([3af4fcf](https://github.com/dandxy89/lp_parser_rs/commit/3af4fcf18a388140faf324bd5f2459aef65d9f75))
* release 1.3.1 ([38f57f6](https://github.com/dandxy89/lp_parser_rs/commit/38f57f6b6aa98b2c1012a903f628718245959ffc))
* release 1.4.0 ([cce8326](https://github.com/dandxy89/lp_parser_rs/commit/cce8326881b55e8b070f58665812e2b3e40e1624))
* release 2.0.0 ([666d968](https://github.com/dandxy89/lp_parser_rs/commit/666d968d1c47d5e7eff2618f702ecb1da74a1295))
* release 2.3.0 ([efafbca](https://github.com/dandxy89/lp_parser_rs/commit/efafbcae561ea76e020156e5f3d9fc7c273e4156))
* release 2.4.0 ([63fd277](https://github.com/dandxy89/lp_parser_rs/commit/63fd2773fe2375b2e2f95b603287cdf1683934f6))
* release 3.0.0 ([91c5c59](https://github.com/dandxy89/lp_parser_rs/commit/91c5c592ed015cecbf573b900ddd3db2980c1aab))
* release 3.0.2 ([e6771bd](https://github.com/dandxy89/lp_parser_rs/commit/e6771bd29fda5fdb7fa4c424b7892b8fbd9678ea))
* release main ([948ff5d](https://github.com/dandxy89/lp_parser_rs/commit/948ff5d43f3cecd2aa5bae41c8ae42a44407a38d))
* release main ([#109](https://github.com/dandxy89/lp_parser_rs/issues/109)) ([fb3f7a4](https://github.com/dandxy89/lp_parser_rs/commit/fb3f7a45a5414958a9aff22369122616cd1e7a2d))
* release main ([#110](https://github.com/dandxy89/lp_parser_rs/issues/110)) ([c5c8e92](https://github.com/dandxy89/lp_parser_rs/commit/c5c8e920ec8f45eb65de28f4c45e412e83dc1d4e))
* release main ([#113](https://github.com/dandxy89/lp_parser_rs/issues/113)) ([8fd7207](https://github.com/dandxy89/lp_parser_rs/commit/8fd72073d046c6efb5a11acc58a9be1b3133faea))
* release main ([#115](https://github.com/dandxy89/lp_parser_rs/issues/115)) ([2020698](https://github.com/dandxy89/lp_parser_rs/commit/202069826a905432cb549a0887e3193d868f0fea))
* release main ([#118](https://github.com/dandxy89/lp_parser_rs/issues/118)) ([dc6c4ce](https://github.com/dandxy89/lp_parser_rs/commit/dc6c4cefc9c49d4658837748602cb68be4d47449))
* release main ([#122](https://github.com/dandxy89/lp_parser_rs/issues/122)) ([543778d](https://github.com/dandxy89/lp_parser_rs/commit/543778d301df1f09a261a05936f5cf1b4f6fec6b))
* release main ([#124](https://github.com/dandxy89/lp_parser_rs/issues/124)) ([d26c112](https://github.com/dandxy89/lp_parser_rs/commit/d26c112e0f5fea7afde6daaeb5f930e983f936ad))
* release main ([#126](https://github.com/dandxy89/lp_parser_rs/issues/126)) ([128cab8](https://github.com/dandxy89/lp_parser_rs/commit/128cab84d7a29f173c181ec350fbbff5e60c56f7))
* release main ([#127](https://github.com/dandxy89/lp_parser_rs/issues/127)) ([3e5c682](https://github.com/dandxy89/lp_parser_rs/commit/3e5c6823cf8eba39439ee544302bc80216a285de))
* release main ([#129](https://github.com/dandxy89/lp_parser_rs/issues/129)) ([22ff9a3](https://github.com/dandxy89/lp_parser_rs/commit/22ff9a3c52f3fcbcafd30adaf9d4286a03a9329a))
* release main ([#132](https://github.com/dandxy89/lp_parser_rs/issues/132)) ([46bc918](https://github.com/dandxy89/lp_parser_rs/commit/46bc918758922fef8eca511b885fdbfdce147663))
* release main ([#137](https://github.com/dandxy89/lp_parser_rs/issues/137)) ([d9567af](https://github.com/dandxy89/lp_parser_rs/commit/d9567af173e37f054e867e11b9e6446589ec56e4))
* release main ([#143](https://github.com/dandxy89/lp_parser_rs/issues/143)) ([9cf65f6](https://github.com/dandxy89/lp_parser_rs/commit/9cf65f613fe77438de27b35dd9c11c4dffc1bd02))
* release main ([#145](https://github.com/dandxy89/lp_parser_rs/issues/145)) ([40909f9](https://github.com/dandxy89/lp_parser_rs/commit/40909f9d20ea675b179dc3402e7a65c7b62cab3b))
* release main ([#148](https://github.com/dandxy89/lp_parser_rs/issues/148)) ([0d7c2eb](https://github.com/dandxy89/lp_parser_rs/commit/0d7c2ebdd8675610ff63e10b81642c022bb685e7))
* release main ([#149](https://github.com/dandxy89/lp_parser_rs/issues/149)) ([cfa1f2c](https://github.com/dandxy89/lp_parser_rs/commit/cfa1f2c39ceeb5aa61c2f332b489f78d95ef17f2))
* release main ([#153](https://github.com/dandxy89/lp_parser_rs/issues/153)) ([96baff8](https://github.com/dandxy89/lp_parser_rs/commit/96baff8997a87f3e78c4ac390c042efebf8270e4))
* release main ([#155](https://github.com/dandxy89/lp_parser_rs/issues/155)) ([60c66d8](https://github.com/dandxy89/lp_parser_rs/commit/60c66d8f780407f7c818978c941a0c5592d74984))
* release main ([#159](https://github.com/dandxy89/lp_parser_rs/issues/159)) ([71b8c7b](https://github.com/dandxy89/lp_parser_rs/commit/71b8c7b5c0cdb868dca7cbe1f9e37dd218cef58e))
* release main ([#164](https://github.com/dandxy89/lp_parser_rs/issues/164)) ([8a8e267](https://github.com/dandxy89/lp_parser_rs/commit/8a8e2671887a8983ed009f0862fbe56784e1569f))
* release main ([#168](https://github.com/dandxy89/lp_parser_rs/issues/168)) ([a0b8eba](https://github.com/dandxy89/lp_parser_rs/commit/a0b8eba2fc5c035793a3de98cafe8b6df72d2d50))
* release main ([#171](https://github.com/dandxy89/lp_parser_rs/issues/171)) ([5fe0bcd](https://github.com/dandxy89/lp_parser_rs/commit/5fe0bcd1d6197d3265ce4a8a09b992aa7a638adf))
* release main ([#181](https://github.com/dandxy89/lp_parser_rs/issues/181)) ([47d9efe](https://github.com/dandxy89/lp_parser_rs/commit/47d9efed7392c46bd22b184d7f10349d30d6f9e2))
* release main ([#187](https://github.com/dandxy89/lp_parser_rs/issues/187)) ([e867c09](https://github.com/dandxy89/lp_parser_rs/commit/e867c09e4553288bb91c770996918048f50961aa))
* release main ([#196](https://github.com/dandxy89/lp_parser_rs/issues/196)) ([99f9f31](https://github.com/dandxy89/lp_parser_rs/commit/99f9f314c67797c55897c285856a9baeb190bfd2))
* set MSRV to 1.88.0 across workspace crates ([071c612](https://github.com/dandxy89/lp_parser_rs/commit/071c612c5f76b37db24119dea8debe8c9332e163))


### Documentation

* fix stale examples and links, fill missing docs, enforce doc lints ([e0bb357](https://github.com/dandxy89/lp_parser_rs/commit/e0bb3577dc3a5c3c5d893b46b3931caad4a45c72))
* refresh READMEs and tag the summary output code fence ([d718289](https://github.com/dandxy89/lp_parser_rs/commit/d7182890167a342508db6d1daaf7d18e9d89d94e))


### Features

* Adding a dedicated TUI for solving, viewing and comparing LP files ([#160](https://github.com/dandxy89/lp_parser_rs/issues/160)) ([8b6c455](https://github.com/dandxy89/lp_parser_rs/commit/8b6c455133f4bc226796fd166d577d57e8b26871))
* Adding the functionality to modify and write to a .lp file ([46c69a9](https://github.com/dandxy89/lp_parser_rs/commit/46c69a997263ab3fbb6bb1ccfd4d6b4dee71bc6a))
* Detailed statistics ([#154](https://github.com/dandxy89/lp_parser_rs/issues/154)) ([e4d74b7](https://github.com/dandxy89/lp_parser_rs/commit/e4d74b7240be508c07ec15998f511e7610c8e5f4))
* Extending Python API, Testing and Typed Files ([#128](https://github.com/dandxy89/lp_parser_rs/issues/128)) ([c457800](https://github.com/dandxy89/lp_parser_rs/commit/c45780087e39799f6901ec46a0b48545b282a3c4))
* Extending python library with runnable main ([8eef3a6](https://github.com/dandxy89/lp_parser_rs/commit/8eef3a62360adef6e35818884e5e2ad475bfb992))
* **python:** add string/MPS parsing, diff and structured variables ([ecbef95](https://github.com/dandxy89/lp_parser_rs/commit/ecbef9570f7ee60037d47ffdfbffa5e57747a584))
* TUI and Perf fixes ([#189](https://github.com/dandxy89/lp_parser_rs/issues/189)) ([c898123](https://github.com/dandxy89/lp_parser_rs/commit/c898123543df4000ab3ce3aca9adcaedb2059a6f))
* tui watch numerics perf ([#193](https://github.com/dandxy89/lp_parser_rs/issues/193)) ([d849d2f](https://github.com/dandxy89/lp_parser_rs/commit/d849d2f6bfa597cdaa59f421f79aa7f9ec571eeb))
* **tui:** Add CSV export of diff report ([fa53f7f](https://github.com/dandxy89/lp_parser_rs/commit/fa53f7fb8108b81cca26f8b0adf25324b5579ea3))


### Bug Fixes

* Adding missing stubs ([180faa5](https://github.com/dandxy89/lp_parser_rs/commit/180faa514a8f10846318c23e33562175c66b2aa8))
* Apply fixes due to ty violations ([76bbebc](https://github.com/dandxy89/lp_parser_rs/commit/76bbebc7ef216386f5f856dbac718cbc4e60cdbd))
* CI Pipeline failure ([3316c6e](https://github.com/dandxy89/lp_parser_rs/commit/3316c6e61c17afa9b32874415ad5582a763c24a6))
* **feat:** Extending the regex and improving the versatility of the original CLI ([#185](https://github.com/dandxy89/lp_parser_rs/issues/185)) ([ebe05b8](https://github.com/dandxy89/lp_parser_rs/commit/ebe05b8cc3361ddf798f6b97c72c290761ad36d7))
* Include README in Python pyproject.toml ([0745fd2](https://github.com/dandxy89/lp_parser_rs/commit/0745fd28202823e633f3b7853e85026ed7e98986))
* **python:** declare dual MIT OR Apache-2.0 licence on PyPI ([de7d14e](https://github.com/dandxy89/lp_parser_rs/commit/de7d14e503f01ceb1b52259283f4049d46a9db42))
* Updating lints ([dc95549](https://github.com/dandxy89/lp_parser_rs/commit/dc955496f611528250061f1ad55f894d73a1912c))


### Performance Improvements

* **python:** parse once, store LpProblem instead of re-parsing text ([eb29882](https://github.com/dandxy89/lp_parser_rs/commit/eb2988218770e39a8f7c08f8eb045fde849f6988))


### Code Refactoring

* Adding comprehensive test cases and improvements to Nom usage ([#123](https://github.com/dandxy89/lp_parser_rs/issues/123)) ([35afeda](https://github.com/dandxy89/lp_parser_rs/commit/35afedad25cc9539774d7e155cafe218d681b5de))
* Clippy ([#150](https://github.com/dandxy89/lp_parser_rs/issues/150)) ([0ec6a51](https://github.com/dandxy89/lp_parser_rs/commit/0ec6a510fe3bd3a9f6f52194de381a110ad19589))
* ponytail audit cleanup ([#195](https://github.com/dandxy89/lp_parser_rs/issues/195)) ([ada04ce](https://github.com/dandxy89/lp_parser_rs/commit/ada04ceebb112725ce75c7fade7bba59c73ab0d8))
* **python:** remove diff CLI and compare(), slim bindings ([65d7873](https://github.com/dandxy89/lp_parser_rs/commit/65d787320e35a6e605ee90c56395c8e54d513b68))
* remove over-engineering flagged by repo audit ([5d3ec69](https://github.com/dandxy89/lp_parser_rs/commit/5d3ec6951243ec27362992240bc839e10753892c))
* strip over-engineering found by repo-wide audit ([c842456](https://github.com/dandxy89/lp_parser_rs/commit/c842456c5fdd1d67ed16fea3dd17d9de71d6393a))
* Use https://github.com/lalrpop/lalrpop ([#144](https://github.com/dandxy89/lp_parser_rs/issues/144)) ([35a001d](https://github.com/dandxy89/lp_parser_rs/commit/35a001dc019013b93ca7df7839f698190278cb2e))


### Styles

* Formatting ([4d3ab97](https://github.com/dandxy89/lp_parser_rs/commit/4d3ab97e560eb04c921ea057da2bfb409c60127f))
* Remove inline imports and qualified imports ([42c20a2](https://github.com/dandxy89/lp_parser_rs/commit/42c20a25b412b6fded358fdca70db4983db6f19b))
* Updating formatting rules ([1d8f526](https://github.com/dandxy89/lp_parser_rs/commit/1d8f526c174d588a1d49fc19e85a89501d91514d))

## [4.0.1](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v4.0.0...parse_lp-v4.0.1) (2026-07-09)


### Bug Fixes

* **python:** declare dual MIT OR Apache-2.0 licence on PyPI ([de7d14e](https://github.com/dandxy89/lp_parser_rs/commit/de7d14e503f01ceb1b52259283f4049d46a9db42))


### Code Refactoring

* remove over-engineering flagged by repo audit ([5d3ec69](https://github.com/dandxy89/lp_parser_rs/commit/5d3ec6951243ec27362992240bc839e10753892c))
* strip over-engineering found by repo-wide audit ([c842456](https://github.com/dandxy89/lp_parser_rs/commit/c842456c5fdd1d67ed16fea3dd17d9de71d6393a))

## [4.0.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v3.5.0...parse_lp-v4.0.0) (2026-07-03)


### ⚠ BREAKING CHANGES

* **python:** parse() now raises LpParseError immediately for malformed files instead of deferring the error to the first attribute access.
* **python:** parse-lp console script and LpParser.compare() are removed; use the Rust CLI's diff subcommand instead.

### Miscellaneous Chores

* enable clippy pedantic workspace-wide and fix all violations ([b334191](https://github.com/dandxy89/lp_parser_rs/commit/b3341919d350ec4ba6d8ef5eb463c8a239a1a0d2))


### Documentation

* fix stale examples and links, fill missing docs, enforce doc lints ([e0bb357](https://github.com/dandxy89/lp_parser_rs/commit/e0bb3577dc3a5c3c5d893b46b3931caad4a45c72))
* refresh READMEs and tag the summary output code fence ([d718289](https://github.com/dandxy89/lp_parser_rs/commit/d7182890167a342508db6d1daaf7d18e9d89d94e))


### Features

* tui watch numerics perf ([#193](https://github.com/dandxy89/lp_parser_rs/issues/193)) ([d849d2f](https://github.com/dandxy89/lp_parser_rs/commit/d849d2f6bfa597cdaa59f421f79aa7f9ec571eeb))


### Performance Improvements

* **python:** parse once, store LpProblem instead of re-parsing text ([eb29882](https://github.com/dandxy89/lp_parser_rs/commit/eb2988218770e39a8f7c08f8eb045fde849f6988))


### Code Refactoring

* ponytail audit cleanup ([#195](https://github.com/dandxy89/lp_parser_rs/issues/195)) ([ada04ce](https://github.com/dandxy89/lp_parser_rs/commit/ada04ceebb112725ce75c7fade7bba59c73ab0d8))
* **python:** remove diff CLI and compare(), slim bindings ([65d7873](https://github.com/dandxy89/lp_parser_rs/commit/65d787320e35a6e605ee90c56395c8e54d513b68))

## [3.5.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v3.4.1...parse_lp-v3.5.0) (2026-06-10)


### Features

* TUI and Perf fixes ([#189](https://github.com/dandxy89/lp_parser_rs/issues/189)) ([c898123](https://github.com/dandxy89/lp_parser_rs/commit/c898123543df4000ab3ce3aca9adcaedb2059a6f))

## [3.4.1](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v3.4.0...parse_lp-v3.4.1) (2026-04-20)


### Build System

* **deps:** bump pygments from 2.19.2 to 2.20.0 in /python ([#180](https://github.com/dandxy89/lp_parser_rs/issues/180)) ([4e4bd0f](https://github.com/dandxy89/lp_parser_rs/commit/4e4bd0f568ce087b832dc727f015eff46fdf555a))
* Updating crates, ty and ruff ([76bbebc](https://github.com/dandxy89/lp_parser_rs/commit/76bbebc7ef216386f5f856dbac718cbc4e60cdbd))


### Miscellaneous Chores

* **deps-dev:** bump basedpyright ([#183](https://github.com/dandxy89/lp_parser_rs/issues/183)) ([95cab0a](https://github.com/dandxy89/lp_parser_rs/commit/95cab0abdb164057fb51f6b2c8adec6fa8f7569c))


### Bug Fixes

* Apply fixes due to ty violations ([76bbebc](https://github.com/dandxy89/lp_parser_rs/commit/76bbebc7ef216386f5f856dbac718cbc4e60cdbd))
* **feat:** Extending the regex and improving the versatility of the original CLI ([#185](https://github.com/dandxy89/lp_parser_rs/issues/185)) ([ebe05b8](https://github.com/dandxy89/lp_parser_rs/commit/ebe05b8cc3361ddf798f6b97c72c290761ad36d7))

## [3.4.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v3.3.1...parse_lp-v3.4.0) (2026-03-02)


### Features

* **tui:** Add CSV export of diff report ([fa53f7f](https://github.com/dandxy89/lp_parser_rs/commit/fa53f7fb8108b81cca26f8b0adf25324b5579ea3))


### Bug Fixes

* CI Pipeline failure ([3316c6e](https://github.com/dandxy89/lp_parser_rs/commit/3316c6e61c17afa9b32874415ad5582a763c24a6))

## [3.3.1](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v3.3.0...parse_lp-v3.3.1) (2026-02-24)


### Continuous Integration

* Fix pipeline sync ([cb8d0bd](https://github.com/dandxy89/lp_parser_rs/commit/cb8d0bda41be746647a7628ab3071f6332b67a4e))
* fix release-please config paths and changelog locations ([5808e72](https://github.com/dandxy89/lp_parser_rs/commit/5808e72bceb4aefd1c0cbe69a4902ebf42dcabc3))
* trigger release-please after tag fix ([bb19a1c](https://github.com/dandxy89/lp_parser_rs/commit/bb19a1cdbac44a957ff8ce07ebbe5e5f729eb2c1))


### Miscellaneous Chores

* release main ([#164](https://github.com/dandxy89/lp_parser_rs/issues/164)) ([8a8e267](https://github.com/dandxy89/lp_parser_rs/commit/8a8e2671887a8983ed009f0862fbe56784e1569f))


### Styles

* Formatting ([4d3ab97](https://github.com/dandxy89/lp_parser_rs/commit/4d3ab97e560eb04c921ea057da2bfb409c60127f))
* Remove inline imports and qualified imports ([42c20a2](https://github.com/dandxy89/lp_parser_rs/commit/42c20a25b412b6fded358fdca70db4983db6f19b))

## [0.2.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v0.2.0...parse_lp-v0.2.0) (2025-01-28)


### Build System

* Upgrading to use nom 0.8.0 ([#112](https://github.com/dandxy89/lp_parser_rs/issues/112)) ([4f8a0e3](https://github.com/dandxy89/lp_parser_rs/commit/4f8a0e326aaad54ad76eb88ebbb2775ea6740454))


### Continuous Integration

* Updating versions ([492517d](https://github.com/dandxy89/lp_parser_rs/commit/492517d9e257608d39aa70cf2bdd8b9e8ca98f5b))


### Miscellaneous Chores

* Override release version ([68a74fc](https://github.com/dandxy89/lp_parser_rs/commit/68a74fc2e8709550571aacd945fd06be2695719c))
* release 1.3.0 ([3af4fcf](https://github.com/dandxy89/lp_parser_rs/commit/3af4fcf18a388140faf324bd5f2459aef65d9f75))
* release 1.3.1 ([38f57f6](https://github.com/dandxy89/lp_parser_rs/commit/38f57f6b6aa98b2c1012a903f628718245959ffc))
* release 1.4.0 ([cce8326](https://github.com/dandxy89/lp_parser_rs/commit/cce8326881b55e8b070f58665812e2b3e40e1624))
* release 2.0.0 ([666d968](https://github.com/dandxy89/lp_parser_rs/commit/666d968d1c47d5e7eff2618f702ecb1da74a1295))
* release 2.3.0 ([efafbca](https://github.com/dandxy89/lp_parser_rs/commit/efafbcae561ea76e020156e5f3d9fc7c273e4156))
* release main ([#109](https://github.com/dandxy89/lp_parser_rs/issues/109)) ([fb3f7a4](https://github.com/dandxy89/lp_parser_rs/commit/fb3f7a45a5414958a9aff22369122616cd1e7a2d))
* release main ([#110](https://github.com/dandxy89/lp_parser_rs/issues/110)) ([c5c8e92](https://github.com/dandxy89/lp_parser_rs/commit/c5c8e920ec8f45eb65de28f4c45e412e83dc1d4e))
* release main ([#113](https://github.com/dandxy89/lp_parser_rs/issues/113)) ([8fd7207](https://github.com/dandxy89/lp_parser_rs/commit/8fd72073d046c6efb5a11acc58a9be1b3133faea))

## [0.1.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v0.1.0...parse_lp-v0.1.0) (2025-01-28)


### Build System

* Upgrading to use nom 0.8.0 ([#112](https://github.com/dandxy89/lp_parser_rs/issues/112)) ([4f8a0e3](https://github.com/dandxy89/lp_parser_rs/commit/4f8a0e326aaad54ad76eb88ebbb2775ea6740454))

## [0.1.0](https://github.com/dandxy89/lp_parser_rs/compare/parse_lp-v0.1.0...parse_lp-v0.1.0) (2025-01-18)


### Miscellaneous Chores

* Override release version ([68a74fc](https://github.com/dandxy89/lp_parser_rs/commit/68a74fc2e8709550571aacd945fd06be2695719c))
* release 1.3.0 ([3af4fcf](https://github.com/dandxy89/lp_parser_rs/commit/3af4fcf18a388140faf324bd5f2459aef65d9f75))
* release 1.3.1 ([38f57f6](https://github.com/dandxy89/lp_parser_rs/commit/38f57f6b6aa98b2c1012a903f628718245959ffc))
* release 1.4.0 ([cce8326](https://github.com/dandxy89/lp_parser_rs/commit/cce8326881b55e8b070f58665812e2b3e40e1624))
* release 2.0.0 ([666d968](https://github.com/dandxy89/lp_parser_rs/commit/666d968d1c47d5e7eff2618f702ecb1da74a1295))
* release main ([#109](https://github.com/dandxy89/lp_parser_rs/issues/109)) ([fb3f7a4](https://github.com/dandxy89/lp_parser_rs/commit/fb3f7a45a5414958a9aff22369122616cd1e7a2d))

## [0.1.0](https://github.com/dandxy89/lp_parser_rs/compare/v0.1.0...v0.1.0) (2025-01-18)
