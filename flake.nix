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
            alias setup-regtest='./tests/regtest/setup-regtest.sh'
            alias setup-mutinynet='./tests/mutinynet/setup-mutinynet.sh'
            alias resume-mutinynet='./scripts/resume-mutinynet.sh'

            # Stop commands
            alias stop='./scripts/stop-services.sh'
            alias stop-regtest='./scripts/stop-regtest.sh'
            alias stop-mutinynet='./scripts/stop-mutinynet.sh'
            alias stop-services='./scripts/stop-services.sh'

            # Explorer commands
            alias explorer-regtest='./target/release/coins-explorer --config config/explorer-regtest.toml'
            alias explorer-mutinynet='./target/release/coins-explorer --config config/explorer-mutinynet.toml'

            # Wallet commands
            alias wallet-regtest='INDEXER_URL=http://localhost:8084 PUBLISHER_URL=http://localhost:8080 cargo run --release -p coins-wallet'
            alias wallet-mutinynet='INDEXER_URL=http://localhost:8083 PUBLISHER_URL=http://localhost:8082 cargo run --release -p coins-wallet'

            # Send tokens between accounts
            # Usage: send <network> <from> <to> [amount] [token_id]
            # Examples: send regtest alice bob 100
            #           send mutinynet genesis alice 1000 1
            send() {
              local network="$1"
              local from="$2"
              local to="$3"
              local amount=''${4:-100}
              local token_id=''${5:-0}

              if [ -z "$from" ] || [ -z "$to" ]; then
                echo "Usage: send <network> <from> <to> [amount] [token_id]"
                echo "  network: regtest | mutinynet"
                echo "  from: genesis | alice | bob"
                echo "  to: genesis | alice | bob | <64-char pubkey>"
                echo "  amount: tokens to send (default: 100)"
                echo "  token_id: token ID to send (default: 0 for native)"
                echo ""
                echo "Examples:"
                echo "  send regtest alice bob 100           # Alice sends 100 native tokens to Bob"
                echo "  send mutinynet genesis alice 1000 1  # Genesis sends 1000 token_id=1 to Alice"
                echo "  send regtest bob abc123...def 50     # Bob sends to specific pubkey"
                return 1
              fi

              # Determine network settings
              local keydir publisher_url
              case "$network" in
                regtest)
                  keydir=".data/regtest/test-keys"
                  publisher_url="http://localhost:8080"
                  ;;
                mutinynet)
                  keydir=".data/mutinynet/keys"
                  publisher_url="http://localhost:8082"
                  ;;
                *)
                  echo "Error: Invalid network '$network'. Use 'regtest' or 'mutinynet'."
                  return 1
                  ;;
              esac

              # Get sender's keyfile
              local sender_keyfile
              case "$from" in
                genesis)
                  sender_keyfile="$keydir/genesis_sk.hex"
                  ;;
                alice)
                  sender_keyfile="$keydir/alice_sk.hex"
                  ;;
                bob)
                  sender_keyfile="$keydir/bob_sk.hex"
                  ;;
                *)
                  echo "Error: Invalid sender '$from'. Use 'genesis', 'alice', or 'bob'."
                  return 1
                  ;;
              esac

              # Get recipient's public key
              local recipient_pk
              case "$to" in
                genesis)
                  recipient_pk=$(./target/release/examples/get_pk "$keydir/genesis_sk.hex")
                  ;;
                alice)
                  recipient_pk=$(./target/release/examples/get_pk "$keydir/alice_sk.hex")
                  ;;
                bob)
                  recipient_pk=$(./target/release/examples/get_pk "$keydir/bob_sk.hex")
                  ;;
                *)
                  # Assume it's a public key
                  if [[ "$to" =~ ^[a-fA-F0-9]{64}$ ]]; then
                    recipient_pk="$to"
                  else
                    echo "Error: Invalid recipient '$to'. Use 'genesis', 'alice', 'bob', or a 64-char hex pubkey."
                    return 1
                  fi
                  ;;
              esac

              # Send the transaction
              ./target/release/coins-client --keyfile "$sender_keyfile" --publisher-url "$publisher_url" send --recipient-pk "$recipient_pk" --amount "$amount" --token-id "$token_id"
            }

            # Mine Bitcoin blocks (regtest only)
            # Usage: mine [count]
            # Examples: mine, mine 5
            mine() {
              local count=''${1:-1}
              local rpc_user="user"
              local rpc_pass="password"
              local rpc_port="18443"

              # Get publisher address from API
              local fee_addr
              fee_addr=$(curl -s http://localhost:8080/address 2>/dev/null | jq -r '.address' 2>/dev/null)

              if [ -z "$fee_addr" ] || [ "$fee_addr" == "null" ]; then
                echo "Error: Could not get publisher address. Is the publisher running?"
                echo "  Start with: setup-regtest"
                return 1
              fi

              echo "Mining $count block(s) to $fee_addr..."
              bitcoin-cli -regtest -rpcuser="$rpc_user" -rpcpassword="$rpc_pass" -rpcport="$rpc_port" \
                generatetoaddress "$count" "$fee_addr" > /dev/null

              if [ $? -eq 0 ]; then
                local height
                height=$(bitcoin-cli -regtest -rpcuser="$rpc_user" -rpcpassword="$rpc_pass" -rpcport="$rpc_port" getblockcount)
                echo "✓ Mined $count block(s). Current height: $height"
                echo "  Waiting 3s for indexer..."
                sleep 3
                echo "✓ Done"
              else
                echo "Error: Failed to mine blocks. Is bitcoind running?"
                return 1
              fi
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
            echo "Wallet:"
            echo "  wallet-regtest           - Start wallet for regtest"
            echo "  wallet-mutinynet         - Start wallet for mutinynet"
            echo ""
            echo "Transactions:"
            echo "  send <net> <from> <to> [amt] [tkn]   - Send tokens between accounts"
            echo "    from/to: genesis, alice, bob, or pubkey (64 hex chars)"
            echo "    Examples: send regtest alice bob 100"
            echo "              send mutinynet genesis alice 1000 1"
            echo ""
            echo "Mining (regtest only):"
            echo "  mine [count]                         - Mine Bitcoin blocks"
            echo "    Examples: mine        # Mine 1 block"
            echo "              mine 5      # Mine 5 blocks"
            echo ""
          '';
        };
      }
    );
}
