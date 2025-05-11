#!/bin/bash

set -e
export RUST_BACKTRACE=1
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../../.. && pwd)"
BE="$ROOT/backend"
FE="$ROOT/frontend"

mkdir -p $ROOT/.dfx/local/canisters/$1

cd $BE
cargo build --target wasm32-unknown-unknown --release --package canister_$1 --locked

cp "target/wasm32-unknown-unknown/release/canister_$1.wasm" $ROOT/.dfx/local/canisters/$1/$1.wasm

candid-extractor "$ROOT/.dfx/local/canisters/$1/$1.wasm" > "$ROOT/.dfx/local/canisters/$1/$1.did"

mkdir -p $FE/src/declarations/$1

didc bind $ROOT/.dfx/local/canisters/$1/$1.did  -t js > $FE/src/declarations/$1/$1.did.js
didc bind $ROOT/.dfx/local/canisters/$1/$1.did  -t ts > $FE/src/declarations/$1/$1.did.t.ts

