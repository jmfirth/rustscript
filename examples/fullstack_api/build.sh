#!/bin/bash
set -e
echo "Building RustScript API + generating types..."
rsc build --emit-types frontend/src/types/
echo "Types generated."
echo ""
echo "Start the API:  rsc run"
echo "Then use the typed API client in your frontend."
