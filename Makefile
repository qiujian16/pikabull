.PHONY: dev build clean install check fmt lint

# Development
dev:
	npm run tauri dev

# Build release .dmg / .app
build:
	npm run tauri build

# Build release with verbose Rust output
build-verbose:
	npm run tauri build -- -- --verbose

# Type-check frontend only
check-frontend:
	npx vue-tsc --noEmit

# Check Rust only
check-rust:
	cd src-tauri && cargo check

# Check both
check: check-frontend check-rust

# Format
fmt:
	cd src-tauri && cargo fmt
	npx prettier --write "src/**/*.{vue,ts,js}"

# Lint Rust
lint:
	cd src-tauri && cargo clippy -- -D warnings

# Clean build artifacts
clean:
	cd src-tauri && cargo clean
	rm -rf dist

# Install dependencies
install:
	npm install
	cd src-tauri && cargo fetch

# Open the built app (macOS)
open:
	open src-tauri/target/release/bundle/dmg/*.dmg
