# Release Asset Checklist

For each Aeroric GitHub Release tag `vX.Y.Z`, the release must include the
desktop installer artifacts produced by `.github/workflows/release-desktop.yml`.
Do not treat a release with only GitHub-generated source archives as complete.

Expected installer assets:

- `Aeroric-X.Y.Z-1.x86_64.rpm`
- `Aeroric_X.Y.Z_aarch64.dmg`
- `Aeroric_X.Y.Z_amd64.deb`
- `Aeroric_X.Y.Z_arm64-setup.exe`
- `Aeroric_X.Y.Z_arm64_en-US.msi`
- `Aeroric_X.Y.Z_x64-setup.exe`
- `Aeroric_X.Y.Z_x64.dmg`
- `Aeroric_X.Y.Z_x64_en-US.msi`

Before closing release work:

1. Verify all eight installer assets are attached to the GitHub Release.
2. Verify or record a `sha256` checksum for each installer asset.
3. Confirm the generated source archives are not the only published files.
