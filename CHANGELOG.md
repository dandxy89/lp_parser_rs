# Changelog

## 0.1.2 (2023-11-14)


### Continuous Integration

* Updating CI Action condition ([#3](https://github.com/dandxy89/lp_parser_rs/issues/3)) ([75e3fe0](https://github.com/dandxy89/lp_parser_rs/commit/75e3fe057ce8461e339c4697bf9790aec56ccd84))


### Miscellaneous Chores

* release 0.1.2 ([51d5a2f](https://github.com/dandxy89/lp_parser_rs/commit/51d5a2f8b7450617051099c7e8bb7c724546e8c3))


### Features

* Adding implementation for Objectives ([#2](https://github.com/dandxy89/lp_parser_rs/issues/2)) ([9de11e1](https://github.com/dandxy89/lp_parser_rs/commit/9de11e1e721cbd8e2c8831eaf6b72650b6ac0947))
* Adding LICENSEs and updating Cargo.toml ([e5938b8](https://github.com/dandxy89/lp_parser_rs/commit/e5938b8aa72c12c7ea627d2e14d06f69a71b770b))
* Adding method to extract constraints ([d7b827d](https://github.com/dandxy89/lp_parser_rs/commit/d7b827d744b96288d350af7ae3689aa337adcfd6))
* Adding semi-continous ([bad2021](https://github.com/dandxy89/lp_parser_rs/commit/bad2021754793ebdf9980029f0053244d527a87f))
* Adding serde as an optional dependency ([7e78069](https://github.com/dandxy89/lp_parser_rs/commit/7e78069ad4643a7ba099088b13344546425f0de1))
* Capture problem name from comments section ([e816dab](https://github.com/dandxy89/lp_parser_rs/commit/e816dabdfe2b0558bac646dcb28a41378bf54728))
* Extending Float such that failing tests pass ([94ed72b](https://github.com/dandxy89/lp_parser_rs/commit/94ed72bc8a072541af835e9a3fede160b59f2f8a))
* Gather variables names ([11d926d](https://github.com/dandxy89/lp_parser_rs/commit/11d926dfb12a1807c2990789d34b27a158b2345b))
* Init the project ([e916cb5](https://github.com/dandxy89/lp_parser_rs/commit/e916cb570bdf789ed7c295febf123db0127390b2))
* Make use of the Atomic operator, remove trim and resolve issue with variables on same line in Bounds sections ([d9db27f](https://github.com/dandxy89/lp_parser_rs/commit/d9db27fdceec2b585b9e34d19993999415f72311))
* Updating .pest file to better handle Bounds ([73c7499](https://github.com/dandxy89/lp_parser_rs/commit/73c7499f5e72bbe59c7edaa81100f528afc1e05b))
* Updating .pest grammar to support Special Order Sets ([191a55e](https://github.com/dandxy89/lp_parser_rs/commit/191a55e425c096268148cba8a57bfef44f996ea5))
* Updating model and compose fn to support SOS constraints ([b955db1](https://github.com/dandxy89/lp_parser_rs/commit/b955db1baa51d3356c4ae6123239480fc480eb73))
* Updating PartialEq and Eq for all models ([a627902](https://github.com/dandxy89/lp_parser_rs/commit/a6279025132debdc6e3ca8166c8b0545dce284ae))


### Bug Fixes

* Adding additional test cases and fixes for .pest ([ea5e188](https://github.com/dandxy89/lp_parser_rs/commit/ea5e1880f23bf6ac5bae891daba4ea7c910ecaef))
* Allow zero or more colons in constraints ([51be6aa](https://github.com/dandxy89/lp_parser_rs/commit/51be6aa9647d9418ae0c6f912e03eea98a8422e0))


### Code Refactoring

* Breaking out the model.rs file into smaller files ([9470c48](https://github.com/dandxy89/lp_parser_rs/commit/9470c4877f05aabe22b853a816862c78521772e9))
* Convert constraints to constraints: HashMap&lt;String, Constraint&gt; ([2e73bb8](https://github.com/dandxy89/lp_parser_rs/commit/2e73bb8af34832fbc66425c0b07b46f1e013ddbe))
* Rename to RuleExt ([0d80542](https://github.com/dandxy89/lp_parser_rs/commit/0d80542af81c133256171f2c7d335bb2244dbdcd))
* Renaming the project ([edd6617](https://github.com/dandxy89/lp_parser_rs/commit/edd6617cab47868a09f5260de8668dfd15df9220))

## 0.1.1 (2023-11-13)

### Bug Fixes

* Allow zero or more colons in constraints
* Adding additional test cases and fixes for .pest

## 0.1.0 (2023-11-13)

### Continuous Integration

* Updating CI Action condition ([#3](https://github.com/dandxy89/lp_parser_rs/issues/3)) ([75e3fe0](https://github.com/dandxy89/lp_parser_rs/commit/75e3fe057ce8461e339c4697bf9790aec56ccd84))

### Features

* Adding implementation for Objectives ([#2](https://github.com/dandxy89/lp_parser_rs/issues/2)) ([9de11e1](https://github.com/dandxy89/lp_parser_rs/commit/9de11e1e721cbd8e2c8831eaf6b72650b6ac0947))
* Adding LICENSEs and updating Cargo.toml ([e5938b8](https://github.com/dandxy89/lp_parser_rs/commit/e5938b8aa72c12c7ea627d2e14d06f69a71b770b))
* Adding method to extract constraints ([d7b827d](https://github.com/dandxy89/lp_parser_rs/commit/d7b827d744b96288d350af7ae3689aa337adcfd6))
* Adding semi-continous ([bad2021](https://github.com/dandxy89/lp_parser_rs/commit/bad2021754793ebdf9980029f0053244d527a87f))
* Adding serde as an optional dependency ([7e78069](https://github.com/dandxy89/lp_parser_rs/commit/7e78069ad4643a7ba099088b13344546425f0de1))
* Capture problem name from comments section ([e816dab](https://github.com/dandxy89/lp_parser_rs/commit/e816dabdfe2b0558bac646dcb28a41378bf54728))
* Extending Float such that failing tests pass ([94ed72b](https://github.com/dandxy89/lp_parser_rs/commit/94ed72bc8a072541af835e9a3fede160b59f2f8a))
* Gather variables names ([11d926d](https://github.com/dandxy89/lp_parser_rs/commit/11d926dfb12a1807c2990789d34b27a158b2345b))
* Init the project ([e916cb5](https://github.com/dandxy89/lp_parser_rs/commit/e916cb570bdf789ed7c295febf123db0127390b2))
* Make use of the Atomic operator, remove trim and resolve issue with variables on same line in Bounds sections ([d9db27f](https://github.com/dandxy89/lp_parser_rs/commit/d9db27fdceec2b585b9e34d19993999415f72311))
* Updating .pest file to better handle Bounds ([73c7499](https://github.com/dandxy89/lp_parser_rs/commit/73c7499f5e72bbe59c7edaa81100f528afc1e05b))
* Updating .pest grammar to support Special Order Sets ([191a55e](https://github.com/dandxy89/lp_parser_rs/commit/191a55e425c096268148cba8a57bfef44f996ea5))
* Updating model and compose fn to support SOS constraints ([b955db1](https://github.com/dandxy89/lp_parser_rs/commit/b955db1baa51d3356c4ae6123239480fc480eb73))
* Updating PartialEq and Eq for all models ([a627902](https://github.com/dandxy89/lp_parser_rs/commit/a6279025132debdc6e3ca8166c8b0545dce284ae))

### Code Refactoring

* Breaking out the model.rs file into smaller files ([9470c48](https://github.com/dandxy89/lp_parser_rs/commit/9470c4877f05aabe22b853a816862c78521772e9))
* Convert constraints to constraints: HashMap&lt;String, Constraint&gt; ([2e73bb8](https://github.com/dandxy89/lp_parser_rs/commit/2e73bb8af34832fbc66425c0b07b46f1e013ddbe))
* Rename to RuleExt ([0d80542](https://github.com/dandxy89/lp_parser_rs/commit/0d80542af81c133256171f2c7d335bb2244dbdcd))
* Renaming the project ([edd6617](https://github.com/dandxy89/lp_parser_rs/commit/edd6617cab47868a09f5260de8668dfd15df9220))
