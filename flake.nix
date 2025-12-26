{
  description = "fit-activities-rerun";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];
      pkgs = import nixpkgs {
        inherit system overlays;
      };
      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
    in
      with pkgs; {
        devShells.default = mkShell rec {
          buildInputs =
            [
              # Rust
              rustToolchain

              # Python
              python3
              uv

              # misc. libraries
              pkg-config
              zlib

              # nix
              nixd
              alejandra

              freetype

              # misc
              typos
              just
            ]
            ++ lib.optionals stdenv.isLinux [
              # GUI libs
              libxkbcommon
              libGL
              fontconfig

              # wayland libraries
              wayland

              # graphics and vulkan
              mesa
              vulkan-loader

              # x11 libraries
              xorg.libXcursor
              xorg.libXrandr
              xorg.libXi
              xorg.libX11
            ];

          shellHook = ''
            # Set LD_LIBRARY_PATH first for compiled packages
            export LD_LIBRARY_PATH=${lib.makeLibraryPath buildInputs}:${lib.makeLibraryPath [stdenv.cc.cc]}:$LD_LIBRARY_PATH

            # Create virtual environment if it doesn't exist
            if [ ! -d ".venv" ]; then
                uv sync
            fi
            source .venv/bin/activate
            uv sync

          '';
        };
      });
}
