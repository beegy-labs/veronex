.PHONY: check test lint ci fmt

check:
	cargo check --workspace
	cd web && npx tsc --noEmit

test:
	cargo test --workspace
	cd web && npx vitest run

lint:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all -- --check

ci: fmt check lint test
