#!/bin/bash
#
# setup-hooks.sh — Install git hooks for QuantumVault development
#
# Copies hooks from scripts/hooks/ to .git/hooks/ and makes them executable.
# Run this once after cloning the repository.
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
GIT_HOOKS_DIR="${PROJECT_ROOT}/.git/hooks"
LOCAL_HOOKS_DIR="${SCRIPT_DIR}/hooks"

# Verify we're in a git repository
if [[ ! -d "${PROJECT_ROOT}/.git" ]]; then
    echo "Error: Not a git repository. Run this script from the project root or a subdirectory."
    exit 1
fi

# Verify hooks directory exists
if [[ ! -d "${LOCAL_HOOKS_DIR}" ]]; then
    echo "Error: ${LOCAL_HOOKS_DIR} not found. Check that scripts/hooks/ exists."
    exit 1
fi

echo "Setting up git hooks for QuantumVault..."
echo "  Local hooks: ${LOCAL_HOOKS_DIR}"
echo "  Git hooks:   ${GIT_HOOKS_DIR}"
echo ""

# Install each hook
HOOKS_INSTALLED=0
HOOKS_FAILED=0

for hook_file in "${LOCAL_HOOKS_DIR}"/*; do
    if [[ -f "${hook_file}" ]]; then
        hook_name=$(basename "${hook_file}")
        target="${GIT_HOOKS_DIR}/${hook_name}"

        # Copy the hook
        if cp "${hook_file}" "${target}"; then
            # Make it executable
            if chmod +x "${target}"; then
                echo "  ✓ Installed ${hook_name}"
                ((HOOKS_INSTALLED++))
            else
                echo "  ✗ Failed to make ${hook_name} executable"
                ((HOOKS_FAILED++))
            fi
        else
            echo "  ✗ Failed to copy ${hook_name}"
            ((HOOKS_FAILED++))
        fi
    fi
done

echo ""
if [[ ${HOOKS_FAILED} -eq 0 ]]; then
    echo "Success! Installed ${HOOKS_INSTALLED} hook(s)."
    echo ""
    echo "Git hooks are now active. Commits and pushes will be validated."
    echo "To bypass hooks in emergencies, use: git commit --no-verify"
    exit 0
else
    echo "Warning: ${HOOKS_INSTALLED} hook(s) installed, ${HOOKS_FAILED} failed."
    exit 1
fi
