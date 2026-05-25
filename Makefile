default: help

.PHONY: help build test fmt lint

help: ## list makefile targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

build: ## build release binary (target/release/watchxy)
	cargo build --release

test: ## run rust tests
	cargo test

fmt: ## format rust files
	cargo fmt

lint: ## lint rust files
	cargo clippy
