.DEFAULT_GOAL := help

VENV := .venv
MATURIN_VERSION := $(shell grep 'requires =' pyproject.toml | cut -d= -f2- | tr -d '[ "]')
PACKAGE_VERSION := $(shell grep version Cargo.toml | head -n 1 | awk '{print $$3}' | tr -d '"' )

# NOTE: make sure to run these in release mode with
MATURIN_EXTRA_ARGS=--release

.PHONY: setup
setup: ## Setup the requirements
	$(info --- Setup dependencies ---)
	uv sync --group dev --no-install-project

.PHONY: build
build: setup ## Build Python binding
	$(info --- Build Python binding ---)
	uvx --from 'maturin[zig]' maturin build $(MATURIN_EXTRA_ARGS)

.PHONY: develop
develop: setup ## Install Python binding
	$(info --- Develop with Python binding ---)
	uvx --from 'maturin[zig]' maturin develop --extras=devel $(MATURIN_EXTRA_ARGS)

.PHONY: install
install: develop ## Install Python binding
	$(info --- Install Python binding ---)
	$(eval TARGET_WHEEL := $(shell ls ../target/wheels/parse_lp-${PACKAGE_VERSION}-*.whl))
	uv pip install $(TARGET_WHEEL) -U

.PHONY: format
format: ## Format the code
	$(info --- Rust format ---)
	cargo fmt
	$(info --- Python format ---)
	uv run --no-sync ruff check . --fix
	uv run --no-sync ruff format .

.PHONY: check-python
check-python: ## Run check on Python
	$(info Check Python format)
	uv run --no-sync ruff format --check --diff .
	$(info Check Python linting)
	uv run --no-sync ruff check .
	$(info Check Python mypy)
	uv run --no-sync mypy
	$(info Check Python pyrefly)
	uv run pyrefly check

.PHONY: unit-test
unit-test: install ## Run unit test
	$(info --- Run Python unit-test ---)
	uv run --no-sync pytest --doctest-modules

.PHONY: clean
clean: ## Run clean
	$(warning --- Clean virtualenv and target directory ---)
	cargo clean
	rm -rf $(VENV)
	find . -type f -name '*.pyc' -delete

.PHONY: help
help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'
