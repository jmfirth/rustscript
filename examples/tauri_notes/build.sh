#!/bin/bash
set -e
echo "Building RustScript backend + generating types..."
rsc build --emit-types frontend/src/types/
echo "Types generated. Build complete."
