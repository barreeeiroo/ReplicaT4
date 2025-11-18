.PHONY: help build run test lint fix clean

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

build: ## Build the project
	cargo build

run: ## Run the server
	cargo run

test: ## Run tests
	cargo test

lint: ## Run clippy linter
	cargo clippy -- -D warnings

fix: ## Auto-fix clippy warnings where possible
	cargo clippy --fix --allow-dirty --allow-staged

format: ## Format code with rustfmt
	cargo fmt

format-check: ## Check code formatting
	cargo fmt -- --check

check: format-check lint test ## Run all checks (format, lint, test)

clean: ## Clean build artifacts
	cargo clean

dev: ## Run in development mode with logging
	RUST_LOG=replicat4=debug cargo run
