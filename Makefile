.PHONY: help docs docs-build test

help:
	@echo "targets:"
	@echo "  docs       build the docs site and serve it locally with live reload"
	@echo "  docs-build build the docs site to docs/book"
	@echo "  test       run the library test suite against throwaway docker databases"

# Serve the mdBook docs (docs/) locally with live reload, opening a browser.
# Same tool the Pages workflow uses; install with `cargo install mdbook` (or grab
# a release from https://github.com/rust-lang/mdBook/releases).
docs:
	@command -v mdbook >/dev/null || { echo "error: mdbook not found; install with 'cargo install mdbook'"; exit 1; }
	mdbook serve docs --open

# Build the static site to docs/book (what CI deploys to Pages).
docs-build:
	@command -v mdbook >/dev/null || { echo "error: mdbook not found; install with 'cargo install mdbook'"; exit 1; }
	mdbook build docs

# Run the migrant_lib suite against throwaway docker postgres/mysql.
test:
	bash migrant_lib/test.sh
