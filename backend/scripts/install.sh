#!/bin/bash

set -e

dfx canister create --all
dfx build bucket
dfx build container

dfx ledger fabricate-cycles --canister container --cycles 9000000000000000
