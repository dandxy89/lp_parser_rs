# Changelog

## [3.0.2](https://github.com/dandxy89/lp_parser_rs/compare/lp_diff-v3.0.2...lp_diff-v3.0.2) (2026-02-24)


### Build System

* Adding release profile ([526ddec](https://github.com/dandxy89/lp_parser_rs/commit/526ddecfa9702cf055daffc2d0771f011cc7d304))


### Continuous Integration

* fix release-please config paths and changelog locations ([5808e72](https://github.com/dandxy89/lp_parser_rs/commit/5808e72bceb4aefd1c0cbe69a4902ebf42dcabc3))


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
* release main ([#164](https://github.com/dandxy89/lp_parser_rs/issues/164)) ([8a8e267](https://github.com/dandxy89/lp_parser_rs/commit/8a8e2671887a8983ed009f0862fbe56784e1569f))


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
