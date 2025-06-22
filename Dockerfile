FROM ghcr.io/openai/codex-universal:latest

# Set environment variables for Rust version
ENV CODEX_ENV_RUST_VERSION=1.85.1

# Set working directory
WORKDIR /workspace/metabolistic3d

# Copy the entire project
COPY . .

CMD ["codex-setup.sh"] 