SHELL := /bin/bash

CARGO ?= /home/ctr/.cargo/bin/cargo
NODE_BIN ?= $(dir $(shell command -v node))
NPM ?= $(shell command -v npm)
FRONTEND_DIR := frontend
FIXTURE ?= deterministic

ANNOTATION_FIXTURES := deterministic multiline hyphenation real-pdf real-pdf-dense
PRODUCT_FIXTURES := experienced-retrieval beginner-retrieval experienced-retrieval-hard experienced-retrieval-novelty-override
PRODUCT_FIXTURE_TARGETS := $(addprefix eval-product-fixture-,$(PRODUCT_FIXTURES))
DISCOVERED_PRODUCT_FIXTURES := $(notdir $(wildcard resource/product-fixtures/*))

export PATH := $(NODE_BIN):$(PATH)

.PHONY: help list-fixtures install build test test-canonical test-product-fixture fmt clippy check debug-annotations debug-canonical eval-product-fixture init-product-fixture export-product-candidates validate-product-labels review-product-labels $(PRODUCT_FIXTURE_TARGETS) run-backend run-frontend dev clean

help:
	@echo "AI Delta Reader commands:"
	@echo "  make install       Install frontend dependencies"
	@echo "  make build         Build backend and frontend"
	@echo "  make test          Run backend tests"
	@echo "  make fmt           Check Rust formatting"
	@echo "  make clippy        Run Rust clippy with warnings denied"
	@echo "  make check         Run fmt, clippy, tests, and frontend build"
	@echo "  make debug-annotations  Run fixture diagnostics (FIXTURE=...) or a capture (FILE=...)"
	@echo "  make debug-canonical    Inspect canonical PDF.js text (PDF=...) or the deterministic fixture"
	@echo "  make eval-product-fixture FIXTURE=name  Evaluate baseline-aware product fit"
	@echo "  make init-product-fixture FIXTURE=name PDF=path  Initialize a real-PDF fixture"
	@echo "  make export-product-candidates FIXTURE=name  Export annotation candidates"
	@echo "  make validate-product-labels FIXTURE=name  Validate human gold labels"
	@echo "  make review-product-labels FIXTURE=name  Generate a label review report"
	@echo "  make list-fixtures  List every supported annotation and product fixture"
	@echo "  make run-backend   Start Rust API server on http://127.0.0.1:8080"
	@echo "  make run-frontend  Start Vite dev server"
	@echo "  make dev           Print commands for running both servers"
	@echo "  make clean         Remove build artifacts"

list-fixtures:
	@echo "Annotation fixtures (make debug-annotations FIXTURE=name):"
	@$(foreach fixture,$(ANNOTATION_FIXTURES),echo "  $(fixture)";)
	@echo "Product fixture directories (annotation drafts may not be evaluation-ready):"
	@$(foreach fixture,$(DISCOVERED_PRODUCT_FIXTURES),echo "  $(fixture)";)
	@echo "Regression product fixtures run by make check:"
	@$(foreach fixture,$(PRODUCT_FIXTURES),echo "  $(fixture)";)

install:
	cd $(FRONTEND_DIR) && $(NPM) install

build:
	$(CARGO) build --workspace
	cd $(FRONTEND_DIR) && $(NPM) run build

test:
	$(CARGO) test

test-canonical:
	node --test tools/pdfjs-extractor/canonical.test.mjs

test-product-fixture: $(PRODUCT_FIXTURE_TARGETS)

$(PRODUCT_FIXTURE_TARGETS): eval-product-fixture-%:
	$(CARGO) run --quiet -p delta-reader-backend --bin product-eval -- "resource/product-fixtures/$*"

fmt:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check: fmt clippy test test-canonical test-product-fixture
	cd $(FRONTEND_DIR) && $(NPM) run test:annotations
	cd $(FRONTEND_DIR) && $(NPM) run build

debug-annotations:
	cd $(FRONTEND_DIR) && $(NPM) run debug:annotations -- $(if $(FILE),--reader-state "$(abspath $(FILE))",--fixture $(FIXTURE))

debug-canonical:
	$(CARGO) run --quiet -p delta-reader-backend --bin canonical-text -- $(if $(PDF),"$(abspath $(PDF))",--fixture tools/pdfjs-extractor/fixtures/text-content.json) $(if $(SENTENCES),--sentences)

eval-product-fixture:
	@test -d "resource/product-fixtures/$(FIXTURE)" || { echo "Unknown product fixture '$(FIXTURE)'. Run 'make list-fixtures' or initialize it first."; exit 2; }
	$(CARGO) run --quiet -p delta-reader-backend --bin product-eval -- "resource/product-fixtures/$(FIXTURE)"

init-product-fixture:
	@test -n "$(FIXTURE)" || { echo "FIXTURE is required"; exit 2; }
	@test -n "$(PDF)" || { echo "PDF is required"; exit 2; }
	$(CARGO) run --quiet -p delta-reader-backend --bin product-fixture -- init "$(FIXTURE)" "$(abspath $(PDF))"

export-product-candidates:
	@test -n "$(FIXTURE)" || { echo "FIXTURE is required"; exit 2; }
	$(CARGO) run --quiet -p delta-reader-backend --bin product-fixture -- export "$(FIXTURE)"

validate-product-labels:
	@$(MAKE) --no-print-directory export-product-candidates FIXTURE="$(FIXTURE)"
	$(CARGO) run --quiet -p delta-reader-backend --bin product-fixture -- validate "$(FIXTURE)"

review-product-labels:
	@$(MAKE) --no-print-directory export-product-candidates FIXTURE="$(FIXTURE)"
	$(CARGO) run --quiet -p delta-reader-backend --bin product-fixture -- review "$(FIXTURE)"

run-backend:
	$(CARGO) run -p delta-reader-backend --bin delta-reader-backend

run-frontend:
	cd $(FRONTEND_DIR) && $(NPM) run dev

dev:
	@echo "Run these in separate terminals:"
	@echo "  make run-backend"
	@echo "  make run-frontend"

clean:
	$(CARGO) clean
	rm -rf $(FRONTEND_DIR)/dist
