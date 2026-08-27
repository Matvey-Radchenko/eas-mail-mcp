#!/bin/sh
set -eu

rustup component add llvm-tools-preview --toolchain 1.95.0
rustup target add aarch64-apple-darwin x86_64-apple-darwin --toolchain 1.95.0
brew install cargo-nextest cargo-deny cargo-llvm-cov cargo-fuzz gitleaks

if ! command -v cargo-mutants >/dev/null 2>&1; then
  if command -v cargo-binstall >/dev/null 2>&1; then
    cargo binstall -y cargo-mutants@27.1.0
  else
  cargo install --locked --version 27.1.0 cargo-mutants
  fi
fi

cargo nextest --version
cargo deny --version
cargo llvm-cov --version
cargo mutants --version
cargo fuzz --version
gitleaks version
