# syntax=docker/dockerfile:1
FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Install GTK3 and other native build dependencies required by gtk-rs crates
RUN export DEBIAN_FRONTEND=noninteractive \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        libgtk-3-dev \
        libglib2.0-dev \
        libgdk-pixbuf-2.0-dev \
        libcairo2-dev \
        libpango1.0-dev \
        libatk1.0-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Ensure cargo bin directory is on PATH for the vscode user
ENV PATH="/home/vscode/.cargo/bin:${PATH}"
