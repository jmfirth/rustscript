#!/bin/bash
set -e
echo "Building RustScript backend + generating types..."
rsc build --emit-types frontend/src/types/
echo "Types generated in frontend/src/types/"
echo ""
echo "In a full Tauri project, you would now run:"
echo "  cd frontend && npm install && npm run build"
echo "  cargo tauri build"
