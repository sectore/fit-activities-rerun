# List available commands
default:
    @just --list

# ── Format ────────────────────────────────────────────────────────────────────

# Format Rust and Python sources
[group('format')]
fmt: fmt-rs fmt-py

# Format Rust sources
[group('format')]
fmt-rs:
    cargo fmt

# Format Python sources
[group('format')]
fmt-py:
    uv run --only-group dev ruff format src/

# Format Markdown sources
[group('format')]
fmt-md:
    dprint fmt

# Check Rust and Python formatting
[group('format')]
fmt-check: fmt-check-rs fmt-check-py

# Check Rust formatting
[group('format')]
fmt-check-rs:
    cargo fmt --check

# Check Python formatting
[group('format')]
fmt-check-py:
    uv run --only-group dev ruff format --check src/

# Check Markdown formatting
[group('format')]
fmt-check-md:
    dprint check

# ── Lint ──────────────────────────────────────────────────────────────────────

# Lint Rust and Python sources
[group('lint')]
lint: lint-rs lint-py

# Lint Rust sources
[group('lint')]
lint-rs:
    cargo clippy -- -D warnings

# Lint Python sources
[group('lint')]
lint-py:
    uv run --only-group dev ruff check src/

# Typecheck Python sources
[group('lint')]
typecheck-py:
    uv run --only-group dev basedpyright src/

# ── Build ─────────────────────────────────────────────────────────────────────

# Build Rust and Python packages
[group('build')]
build: build-rs build-py

# Build Rust binary
[group('build')]
build-rs:
    cargo build

# Build Python package
[group('build')]
build-py:
    uv build

# ── Run ───────────────────────────────────────────────────────────────────────

# Run the Rust app.  Example: just run-rs path/to/activity.fit
[group('run')]
run-rs fit:
    cargo run -- --fit {{fit}}

# Run the Python app.  Example: just run-py path/to/activity.fit
[group('run')]
run-py fit:
    fit-activities-rerun --fit {{fit}}
