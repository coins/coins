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

            # WASM build dependencies
            llvmPackages.lld  # Provides lld linker for WASM
            wasm-pack         # WASM packaging tool

            # Node.js for dev-browser and other tools
            nodejs_22

            # Playwright browser automation (NixOS-compatible)
            playwright-driver.browsers

            # Script utilities
            jq
            curl
            lsof

            # FHS compatibility for scripts with /bin/bash shebang
            steam-run
          ];

          shellHook = ''
            # Workaround for scripts with /bin/bash shebang on NixOS
            # Use: fhs ./script.sh  (runs in FHS-compatible environment)
            alias fhs='steam-run'

            # Set Playwright to use nix-provided browsers
            export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
            export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true

            # Setup commands
            alias setup-regtest='./scripts/setup-regtest.sh'
            alias setup-mutinynet='./scripts/setup-mutinynet.sh'
            alias resume-mutinynet='./scripts/resume-mutinynet.sh'

            # Stop commands
            alias stop='./scripts/stop-services.sh'
            alias stop-regtest='./scripts/stop-regtest.sh'
            alias stop-mutinynet='./scripts/stop-mutinynet.sh'
            alias stop-services='./scripts/stop-services.sh'

            # Explorer commands
            alias explorer-regtest='./target/release/coins-explorer --config config/explorer-regtest.toml'
            alias explorer-mutinynet='./target/release/coins-explorer --config config/explorer-mutinynet.toml'

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
            echo "  Development Environment Ready"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo ""
            echo "Setup:"
            echo "  setup-regtest            - Start local regtest environment"
            echo "  setup-mutinynet          - Start mutinynet environment"
            echo "  resume-mutinynet         - Restart mutinynet services"
            echo ""
            echo "Stop:"
            echo "  stop-services            - Stop all coins services"
            echo "  stop-regtest             - Stop regtest (services + bitcoind + data)"
            echo "  stop-mutinynet           - Stop mutinynet (services + data)"
            echo ""
            echo "Explorer:"
            echo "  explorer-regtest         - Start explorer for regtest"
            echo "  explorer-mutinynet       - Start explorer for mutinynet"
            echo ""
            echo "Transactions:"
            echo "  send <network> [amount]  - Send tokens Alice->Bob"
            echo ""
          '';
        };
      }
    );
}
