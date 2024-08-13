#!/usr/bin/env sh
rustup default nightly
cargo zigbuild --target x86_64-unknown-linux-musl --release --locked
cp -f ./target/x86_64-unknown-linux-musl/release/joshuto ~/.local/bin/cargo/bin/
echo "Installed joshuto: ~/.local/bin/cargo/bin/joshuto"
rustup default stable
