# Strom Project Context
## Architecture
  - **Frontend** (`strom-frontend`): egui-based GUI that
  compiles to both native and WASM
  - **Backend** (`strom-backend`): Axum server that can run the
   native GUI and serve the embedded WASM version

  ## Troubleshooting GUI Issues
  When debugging GUI problems:
  1. Add logging to frontend code
  2. Recompile and restart backend (frontend code is compiled
  into backend binary)
  3. Monitor backend logs - they include both backend and GUI
  logs
