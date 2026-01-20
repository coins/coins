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
            # Send tokens from Alice to Bob
            # Usage: send <network> [amount]
            # Examples: send regtest 100, send mutinynet 500
            send() {
              local network="$1"
              local amount=''${2:-100}

              case "$network" in
                regtest)
                  local BOB_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/bob_sk.hex)
                  ./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex --publisher-url "http://localhost:8080" send --recipient-pk "$BOB_PK" --amount "$amount"
                  ;;
                mutinynet)
                  local BOB_PK=$(./target/release/examples/get_pk .data/mutinynet/keys/bob_sk.hex)
                  ./target/release/coins-client --keyfile .data/mutinynet/keys/alice_sk.hex --publisher-url "http://localhost:8082" send --recipient-pk "$BOB_PK" --amount "$amount"
                  ;;
                *)
                  echo "Usage: send <network> [amount]"
                  echo "  network: regtest | mutinynet"
                  echo "  amount: tokens to send (default: 100)"
                  echo ""
                  echo "Examples:"
                  echo "  send regtest 100"
                  echo "  send mutinynet 500"
                  return 1
                  ;;
              esac
            }

            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo "🦀 Development Environment Ready"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo ""
            echo "Available commands:"
            echo "  bitcoin-cli              - Connect to remote Bitcoin node"
            echo "  send <network> [amount]  - Send tokens Alice->Bob"
            echo ""
            echo "Examples:"
            echo "  send regtest 100"
            echo "  send mutinynet 500"
            echo ""
          '';
        };
      }
    );
}
