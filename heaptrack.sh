#!/usr/bin/env bash
heaptrack ./target/release/vaultless-api
heaptrack_gui heaptrack.vaultless-api.zst
