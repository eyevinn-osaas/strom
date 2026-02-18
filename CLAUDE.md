- **Frontend** (`strom-frontend`): egui-based GUI that
compiles to both native and WASM
- **Backend** (`strom-backend`): Axum server + GUI that can run the
  native GUI and serve the embedded WASM version

## Language
- All code, comments, commit messages, PR titles, PR descriptions, and documentation must be in English

## Security
- Always anonymize sensitive data (IP addresses, hostnames, credentials, internal server names) before including in commits, PRs, or documentation
- Use example.com, 192.0.2.x, or placeholder values instead of real infrastructure data

## Troubleshooting GUI Issues
1. Add logging to strom-frontend
2. Recompile and restart backend. When in native gui mode, default, backend logs shows whole application log

don't suggest blacklisting elements when troubleshooting segfaults. 

do not add emojis to logging.
if you find emojis in log rows, i.e. info!, debug!, trace!, warn!, error!: remove emojis. emojis in icons are ok. 

when investigating pipeline errors, segfaults and troubleshooting:
- Use GST_DEBUG an GST_DEBUG_FILE for gstreamer logs
- Use config logging in .strom.toml, set level to debug or tracing. Then monitor the logging file.
- See /docs for segfaults troubleshooting

## Code Organization
- When working in or near a file that exceeds 1500 lines, proactively suggest splitting it into focused sub-modules (following the pattern used for pipeline.rs and app.rs).
- Each sub-module should have a single clear responsibility (e.g. construction, lifecycle, linking, properties).
- Check for large files with: `find backend/src frontend/src -name "*.rs" | xargs wc -l | sort -rn | head -20`
