#!/usr/bin/env bash
set -e

echo "🦀 Running Clippy auto-fix for all crates..."

cargo clippy --fix --allow-dirty --allow-staged --workspace --all-targets

echo "✅ Clippy fixes applied successfully!"
echo "💡 Run 'git diff' to review the changes before committing."
