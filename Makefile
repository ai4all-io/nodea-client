# Nodea Tauri Application Makefile
# Usage: make <target>
# Run 'make help' to see all available targets

.PHONY: help install install-rust install-node test test-rust test-integration test-all \
        build build-frontend dev clean format check check-rust

# Default target
.DEFAULT_GOAL := help

##@ General

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ Installation

install: install-node install-rust ## Install all dependencies (Node + Rust)
	@echo "✅ All dependencies installed"

install-node: ## Install Node.js dependencies
	@echo "📦 Installing Node.js dependencies..."
	npm install
	@echo "✅ Node.js dependencies installed"

install-rust: ## Install Rust dependencies
	@echo "🦀 Installing Rust dependencies..."
	cd src-tauri && cargo fetch
	@echo "✅ Rust dependencies installed"

##@ Testing

test: test-rust ## Run all tests (alias for test-rust)

test-rust: ## Run Rust unit and integration tests
	@echo "🧪 Running Rust tests..."
	cd src-tauri && cargo test --all-targets
	@echo "✅ Rust tests passed"

test-unit: ## Run only Rust unit tests
	@echo "🧪 Running unit tests..."
	cd src-tauri && cargo test --lib
	@echo "✅ Unit tests passed"

test-integration: ## Run only integration tests
	@echo "🧪 Running integration tests..."
	cd src-tauri && cargo test --test integration_test
	cd src-tauri && cargo test --test model_manager_tests
	@echo "✅ Integration tests passed"

test-with-model: ## Run tests with real GGUF model (set GGUF_MODEL_PATH env var)
	@echo "🧪 Running tests with real GGUF model..."
	@if [ -z "$(GGUF_MODEL_PATH)" ]; then \
		echo "❌ Error: GGUF_MODEL_PATH environment variable not set"; \
		echo "Usage: GGUF_MODEL_PATH=/path/to/model.gguf make test-with-model"; \
		exit 1; \
	fi
	cd src-tauri && GGUF_MODEL_PATH=$(GGUF_MODEL_PATH) cargo test -- --ignored
	@echo "✅ Model tests passed"

test-verbose: ## Run tests with verbose output
	@echo "🧪 Running tests with verbose output..."
	cd src-tauri && cargo test --all-targets -- --nocapture
	@echo "✅ Tests completed"

test-all: test-rust ## Run all available tests
	@echo "✅ All tests completed"

##@ Development

dev: ## Run the app in development mode
	@echo "🚀 Starting Tauri app in development mode..."
	npm run tauri dev

build: ## Build the app for production
	@echo "🔨 Building Tauri app..."
	npm run tauri build
	@echo "✅ Build complete"

build-rust: ## Build only the Rust backend
	@echo "🔨 Building Rust backend..."
	cd src-tauri && cargo build
	@echo "✅ Rust build complete"

build-rust-release: ## Build Rust backend in release mode
	@echo "🔨 Building Rust backend (release)..."
	cd src-tauri && cargo build --release
	@echo "✅ Release build complete"

build-frontend: ## Type-check and build the React frontend
	@echo "🔨 Building frontend..."
	npx tsc --noEmit
	npm run build
	@echo "✅ Frontend build complete"

##@ Code Quality

check: check-rust build-frontend ## Run all checks (Rust + frontend build)

check-rust: ## Check Rust code without building
	@echo "🔍 Checking Rust code..."
	cd src-tauri && cargo check
	@echo "✅ Rust check passed"

clippy: ## Run Clippy linter on Rust code
	@echo "📎 Running Clippy..."
	cd src-tauri && cargo clippy -- -D warnings
	@echo "✅ Clippy passed"

format: ## Format Rust code
	@echo "💅 Formatting Rust code..."
	cd src-tauri && cargo fmt
	@echo "✅ Code formatted"

format-check: ## Check if Rust code is formatted
	@echo "🔍 Checking Rust formatting..."
	cd src-tauri && cargo fmt -- --check
	@echo "✅ Formatting check passed"

licenses: ## Regenerate THIRD_PARTY_LICENSES.md (needs cargo-about and npm ci)
	./scripts/generate-third-party-licenses.sh

##@ Cleanup

clean: ## Clean all build artifacts
	@echo "🧹 Cleaning build artifacts..."
	cd src-tauri && cargo clean
	rm -rf node_modules
	rm -rf dist
	rm -rf src-tauri/target
	@echo "✅ Cleanup complete"

clean-rust: ## Clean only Rust build artifacts
	@echo "🧹 Cleaning Rust artifacts..."
	cd src-tauri && cargo clean
	@echo "✅ Rust cleanup complete"

clean-node: ## Clean only Node.js artifacts
	@echo "🧹 Cleaning Node.js artifacts..."
	rm -rf node_modules
	rm -rf dist
	@echo "✅ Node.js cleanup complete"

##@ Documentation

docs: ## Generate and open Rust documentation
	@echo "📚 Generating Rust documentation..."
	cd src-tauri && cargo doc --open --no-deps
	@echo "✅ Documentation generated"

docs-all: ## Generate documentation including dependencies
	@echo "📚 Generating all documentation..."
	cd src-tauri && cargo doc --open
	@echo "✅ Documentation generated"

##@ Utilities

update: ## Update all dependencies
	@echo "⬆️  Updating dependencies..."
	npm update
	cd src-tauri && cargo update
	@echo "✅ Dependencies updated"

verify: install check test ## Full verification (install + check + test)
	@echo "✅ Full verification complete"

info: ## Show project information
	@echo "📊 Project Information"
	@echo "─────────────────────────────────────"
	@echo "Node version:     $$(node --version)"
	@echo "npm version:      $$(npm --version)"
	@echo "Rust version:     $$(rustc --version)"
	@echo "Cargo version:    $$(cargo --version)"
	@echo "─────────────────────────────────────"
	@echo "Working directory: $$(pwd)"
	@echo "Tauri directory:   $$(pwd)/src-tauri"

##@ CI/CD

ci: check-rust format-check test-rust build-frontend ## Run CI pipeline (check + format + test + frontend)
	@echo "✅ CI pipeline passed"

pre-commit: format check-rust test-unit ## Run pre-commit checks
	@echo "✅ Pre-commit checks passed"

##@ Quick Commands

quick-test: ## Quick test (unit tests only, no integration)
	@echo "⚡ Running quick tests..."
	cd src-tauri && cargo test --lib
	@echo "✅ Quick tests passed"

rebuild: clean-rust build-rust ## Clean and rebuild Rust backend
	@echo "✅ Rebuild complete"
