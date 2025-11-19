.PHONY: help build run test lint fix clean coverage coverage-html coverage-open

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

coverage: ## Run tests with coverage report
	cargo tarpaulin --out Stdout

coverage-html: ## Generate HTML coverage report
	cargo tarpaulin --out Html --output-dir ./coverage

coverage-open: coverage-html ## Generate and open HTML coverage report
	@echo "Opening coverage report..."
	@command -v xdg-open >/dev/null 2>&1 && xdg-open coverage/tarpaulin-report.html || \
	command -v open >/dev/null 2>&1 && open coverage/tarpaulin-report.html || \
	command -v explorer.exe >/dev/null 2>&1 && explorer.exe coverage/tarpaulin-report.html || \
	echo "Coverage report generated at coverage/tarpaulin-report.html"

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
	rm -rf coverage/

dev: ## Run in development mode with logging
	RUST_LOG=replicat4=debug cargo run
