#!/usr/bin/env zsh

(cd ~/Dev/example-terminal-app/ && cargo build)
cp ~/Dev/example-terminal-app/target/debug/example-terminal-app .

if (( $# > 0 )); then
    cargo run -- $@
else
    cargo run -- $PWD/example-terminal-app
fi
