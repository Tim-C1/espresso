SHELL := /bin/bash

CARGO ?= /home/ctr/.cargo/bin/cargo
NODE_BIN ?= /home/ctr/.nvm/versions/node/v26.3.0/bin
NPM ?= $(NODE_BIN)/npm
FRONTEND_DIR := frontend

export PATH := $(NODE_BIN):$(PATH)

.PHONY: help install build test fmt clippy check run-backend run-frontend dev clean

help:
	@echo "AI Delta Reader commands:"
	@echo "  make install       Install frontend dependencies"
	@echo "  make build         Build backend and frontend"
	@echo "  make test          Run backend tests"
	@echo "  make fmt           Check Rust formatting"
	@echo "  make clippy        Run Rust clippy with warnings denied"
	@echo "  make check         Run fmt, clippy, tests, and frontend build"
	@echo "  make run-backend   Start Rust API server on http://127.0.0.1:8080"
	@echo "  make run-frontend  Start Vite dev server"
	@echo "  make dev           Print commands for running both servers"
	@echo "  make clean         Remove build artifacts"

install:
	cd $(FRONTEND_DIR) && $(NPM) install

build:
	$(CARGO) build --workspace
	cd $(FRONTEND_DIR) && $(NPM) run build

test:
	$(CARGO) test

fmt:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check: fmt clippy test
	cd $(FRONTEND_DIR) && $(NPM) run build

run-backend:
	$(CARGO) run -p delta-reader-backend

run-frontend:
	cd $(FRONTEND_DIR) && $(NPM) run dev

dev:
	@echo "Run these in separate terminals:"
	@echo "  make run-backend"
	@echo "  make run-frontend"

clean:
	$(CARGO) clean
	rm -rf $(FRONTEND_DIR)/dist
