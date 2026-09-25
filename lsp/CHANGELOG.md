# Changelog

## [3.0.2](https://github.com/dandxy89/lp_parser_rs/compare/lp-lsp-v0.1.0...lp-lsp-v3.0.2) (2026-09-25)


### Continuous Integration

* trigger release-please after tag fix ([bb19a1c](https://github.com/dandxy89/lp_parser_rs/commit/bb19a1cdbac44a957ff8ce07ebbe5e5f729eb2c1))


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


### Documentation

* correct and rewrite documentation across the library, bindings, TUI, LSP and site ([471b7ff](https://github.com/dandxy89/lp_parser_rs/commit/471b7ff37fd5918eca0e286537dc22431f5cb104))


### Bug Fixes

* **lsp:** adding a LSP ([#213](https://github.com/dandxy89/lp_parser_rs/issues/213)) ([f2d2609](https://github.com/dandxy89/lp_parser_rs/commit/f2d2609bdb482723b1e8fb5d4a239c03f6284d95))
* **lsp:** publish edit diagnostics off the didChange handler ([270b125](https://github.com/dandxy89/lp_parser_rs/commit/270b125d0abcd03165405df95bc8f9ee43729119))
* **lsp:** stop asking pull-mode clients to refresh on every edit ([d0f1897](https://github.com/dandxy89/lp_parser_rs/commit/d0f189735fca01b6d3d2676e2449708263a973b6))
* **lsp:** take the line terminator from the first line ([d4c8261](https://github.com/dandxy89/lp_parser_rs/commit/d4c8261d0cce80110e187d70ce65f6ba5ea78200))


### Performance Improvements

* byte-identical performance improvements for the library and LSP ([#215](https://github.com/dandxy89/lp_parser_rs/issues/215)) ([e9c1b52](https://github.com/dandxy89/lp_parser_rs/commit/e9c1b52ed4d9cca93da6315aeacc19640d17361d))
* **lsp:** cap variable completions and filter them by prefix ([e2d278b](https://github.com/dandxy89/lp_parser_rs/commit/e2d278b3e07e9df2c5ad121d19809347122ce00f))
* **lsp:** descend only into children touching the code action range ([006bc47](https://github.com/dandxy89/lp_parser_rs/commit/006bc471324a584b04d3b0ca556044c9c73b02b6))
* **lsp:** find model names for inlay hints by binary search ([ca2a2a3](https://github.com/dandxy89/lp_parser_rs/commit/ca2a2a37e7db519a7c2cf5e6d4f4aca9ea88a024))
* **lsp:** only re-indent on Enter in documents over 1 MiB ([c6f4c38](https://github.com/dandxy89/lp_parser_rs/commit/c6f4c38325ec7c36b8880236fad9c26668a1d1a1))
* **lsp:** resolve usage code lenses lazily ([e71fc41](https://github.com/dandxy89/lp_parser_rs/commit/e71fc413ddd2779699df46db056348c0e9df1022))
* **lsp:** skip UTF-16 re-encoding on ASCII lines ([6fd9b5d](https://github.com/dandxy89/lp_parser_rs/commit/6fd9b5d84ff78bed8a4aad3bcfc1e069ede1e862))
* **lsp:** update the line index incrementally per edit ([adb6755](https://github.com/dandxy89/lp_parser_rs/commit/adb6755458818deaf80c26fbbfbaf79f56082385))


### Tests

* **lsp:** bench a didChange of 1000 edits on a 200k-constraint model ([ce14fa7](https://github.com/dandxy89/lp_parser_rs/commit/ce14fa70f53148077b32e6608edcf740a39190ea))
* **lsp:** bench code actions at the cursor on a 200k-constraint model ([2ef0812](https://github.com/dandxy89/lp_parser_rs/commit/2ef0812223824a948280fcfce0af0bf7da5c5440))
* **lsp:** bench code lens compute and encode on a 200k-constraint model ([01c7b22](https://github.com/dandxy89/lp_parser_rs/commit/01c7b22e50b6273f6d9e445cbfc4a93cc1bddd9c))
* **lsp:** bench code lens on a 40k-term single-line objective ([d4415a4](https://github.com/dandxy89/lp_parser_rs/commit/d4415a4bbe56d1db1137b8931f654c831ed6e8ee))
* **lsp:** bench completion after a label on a 200k-variable model ([484ac27](https://github.com/dandxy89/lp_parser_rs/commit/484ac2722c85c0bec3b778225d8df2c3ef31214d))
* **lsp:** bench inlay hints for a viewport of a 200k-constraint model ([463d2d9](https://github.com/dandxy89/lp_parser_rs/commit/463d2d964874b8a922264cf0e882d9b294878745))
* **lsp:** bench on-type formatting after Enter on a 200k-constraint model ([64d11df](https://github.com/dandxy89/lp_parser_rs/commit/64d11df54e416991be0a6348b30daf74be71a183))
* **lsp:** wait for diagnostics of the expected version in the e2e session ([76063fe](https://github.com/dandxy89/lp_parser_rs/commit/76063fe5aaaa55ffcb77a0641b666c2dbc4ba437))
