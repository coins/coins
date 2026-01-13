{
  description = "Development environment with Bitcoin";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Bitcoin CLI only (for remote node access)
            bitcoin

            # Rust toolchain
            rustc
            cargo

            # Build dependencies
            pkg-config
            openssl

            # Script utilities
            jq
            curl
            lsof
          ];

          shellHook = ''
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "🦀 Development Environment Ready"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo ""
            echo "Available commands:"
            echo "  bitcoin-cli     - Connect to remote Bitcoin node"
            echo ""
            echo "Example: bitcoin-cli -rpcconnect=<host> -rpcuser=<user> -rpcpassword=<pass> getblockcount"
            echo ""
          '';
        };
      }
    );
}
