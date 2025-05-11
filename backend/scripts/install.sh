#!/bin/bash

set -e

# cargo doesn't work well if we've got errors
dfx build cdn_bucket
dfx build cdn_container

dfx ledger fabricate-cycles --canister cdn_container --cycles 9000000000000000
