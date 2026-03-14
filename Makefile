.PHONY: check test lint ci fmt

check:
	cargo check --workspace &
	cd web && npx tsc --noEmit &
	wait

test:
	cargo nextest run --workspace 2>/dev/null || cargo test --workspace
	cd web && npx vitest run

lint:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all -- --check

ci:
	$(MAKE) fmt
	$(MAKE) -j2 check lint
	$(MAKE) test
