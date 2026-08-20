# Packaged resources

Release jobs stage the verified optional-format worker at
`format-workers/libheif/manifest.json` plus the manifest-named executable. The complete `resources/`
tree maps to the Tauri resource root, so the runtime lookup remains
`$RESOURCE/format-workers/libheif` on every platform.

No generated worker binary or manifest is committed. A build without staged files remains the
intentional core-only application and reports `codec-unavailable` for AVIF/HEIC previews.
