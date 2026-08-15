# Update Rerun SDK + Rust version

## 1. Rerun SDK

### Rust

`crates/fit-activities-rerun/Cargo.toml`:

```diff
-rerun = { version = "0.31.1", ... }
+rerun = { version = "0.33.0", ... }
```

### Python

`pyproject.toml`:

```diff
-"rerun-sdk==0.31.1",
+"rerun-sdk==0.33.0",
```

## 2. Rust version

`rust-toolchain.toml`:

```diff
-channel = "1.94.1"
+channel = "1.96.0"
```

`crates/fit-activities-rerun/Cargo.toml`:

```diff
-rust-version = "1.94.1"
+rust-version = "1.96.0"
```

## 3. Nix

```sh
nix flake update
direnv reload
```

## 4. (other) Python deps

```sh
uv lock --upgrade
uv sync
```

## 5. (other) Rust deps

```sh
cargo upgrade --incompatible
```
