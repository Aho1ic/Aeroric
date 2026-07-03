# Project Notes

## Branch policy

Only commit and push project changes to the `main` branch. Do not create or
push feature, fix, or agent-specific branches for this project unless the user
explicitly asks for a different branch in that same task.

## Frontend changes

All project changes that touch frontend UI or behavior must be made through the
project's frontend tooling and existing component patterns. Do not bypass the
frontend toolchain with unrelated one-off UI implementations.

## GitHub Release assets

When publishing an Aeroric release, do not leave the release with only the
GitHub-generated source archives. Each release tag `vX.Y.Z` should include the
desktop installer artifacts produced by `.github/workflows/release-desktop.yml`:

- `Aeroric-X.Y.Z-1.x86_64.rpm`
- `Aeroric_X.Y.Z_aarch64.dmg`
- `Aeroric_X.Y.Z_amd64.deb`
- `Aeroric_X.Y.Z_arm64-setup.exe`
- `Aeroric_X.Y.Z_arm64_en-US.msi`
- `Aeroric_X.Y.Z_x64-setup.exe`
- `Aeroric_X.Y.Z_x64.dmg`
- `Aeroric_X.Y.Z_x64_en-US.msi`

Before considering a release complete, verify that all expected assets are
present on the GitHub Release and record/check the `sha256` for each uploaded
installer.
