# Joshuto install helper (migrated from install.sh).
#
#   make              build (glibc, all features) and install
#   make install-musl  build for x86_64-unknown-linux-musl and install
#   make clean  remove build artifacts
#   make fmt    format all code (cargo fmt)
#   make check  lint with clippy (deny warnings)

BIN_NAME := joshuto
INSTALL_DIR := $(HOME)/.local/bin/cargo/bin
MUSL_TARGET := x86_64-unknown-linux-musl

.PHONY: all install install-musl clean fmt check help

all: install

install:
	rustup default nightly
	cargo build --release --locked
	cp -f ./target/release/$(BIN_NAME) $(INSTALL_DIR)/
	@echo "Installed $(BIN_NAME): $(INSTALL_DIR)/$(BIN_NAME)"
	rustup default stable

install-musl:
	rustup default nightly
	cargo build --target $(MUSL_TARGET) --release --locked
	cp -f ./target/$(MUSL_TARGET)/release/$(BIN_NAME) $(INSTALL_DIR)/
	@echo "Installed $(BIN_NAME) (musl): $(INSTALL_DIR)/$(BIN_NAME)"
	rustup default stable

clean:
	cargo clean

fmt:
	cargo fmt --all

check:
	cargo clippy --all-targets --all-features -- -D warnings

help:
	@echo "Targets:"
	@echo "  install (default)  build with default features and install to $(INSTALL_DIR)"
	@echo "  install-musl       musl build and install to $(INSTALL_DIR)"
	@echo "  clean              remove build artifacts"
	@echo "  fmt                format all code with cargo fmt"
	@echo "  check              lint with clippy (warnings denied)"
