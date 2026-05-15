## Xullul Release

This fork publishes personal Windows builds as `codex-xullul`. The release path
is intentionally small: build the Windows binary from the local Cargo workspace,
install it locally under the fork-only command name, publish a GitHub Release
with the executable/checksum/installer asset trio, and smoke-test the public
installer.

### Release Checklist

1. Verify the checkout and release scope.

   ```powershell
   cd C:\Users\Keenu\KeenuProjects\codex\codex
   git status --short --branch
   git diff --stat
   ```

2. Commit and push the release candidate.

   ```powershell
   git add -A
   git commit -m "<release-scope>"
   git push origin main
   ```

3. Choose a release version and tag.

   Use dated fork release tags for fork-only builds, for example
   `xullul-v2026.05.09.1`. Verify the tag does not already exist:

   ```powershell
   git tag --list "xullul-v2026.05.09*"
   gh release view xullul-v2026.05.09.1 --repo xullul/codex
   ```

4. Build the binary directly from the Cargo workspace. Do not require `just`.

   ```powershell
   cd C:\Users\Keenu\KeenuProjects\codex\codex\codex-rs
   cargo build -p codex-cli --bin codex --release
   ```

   The output is:

   ```text
   codex-rs\target\release\codex.exe
   ```

5. Install the local fork alias.

   Copy the release build to the fork install location and ensure both the
   `bin` shim and legacy `shim` path delegate to the adjacent fork binary:

   ```powershell
   $version = "2026.05.09.1"
   $sourceExe = "C:\Users\Keenu\KeenuProjects\codex\codex\codex-rs\target\release\codex.exe"
   $installRoot = Join-Path $env:LOCALAPPDATA "Programs\Xullul\Codex"
   $installDir = Join-Path $installRoot "bin"
   New-Item -ItemType Directory -Force -Path $installDir | Out-Null

   $targetExe = Join-Path $installDir "codex-xullul.exe"
   try {
       Copy-Item -LiteralPath $sourceExe -Destination $targetExe -Force
   } catch {
       $targetExe = Join-Path $installDir "codex-xullul-$version.exe"
       Copy-Item -LiteralPath $sourceExe -Destination $targetExe -Force
   }

   foreach ($dir in @($installDir, (Join-Path $installRoot "shim"))) {
       New-Item -ItemType Directory -Force -Path $dir | Out-Null
       $shimPath = Join-Path $dir "codex-xullul.cmd"
       Set-Content -LiteralPath $shimPath -Encoding ASCII -Value "@echo off`r`n`"$targetExe`" %*`r`n"
   }
   ```

   Verify from a fresh PowerShell process:

   ```powershell
   pwsh -NoLogo -NoProfile -Command "Get-Command codex-xullul | Select-Object -ExpandProperty Source; codex-xullul --version"
   ```

6. Package the release assets.

   ```powershell
   cd C:\Users\Keenu\KeenuProjects\codex\codex
   $version = "2026.05.09.1"
   $tag = "xullul-v$version"
   $artifactDir = "target\xullul-release\$tag"
   New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null

   Copy-Item codex-rs\target\release\codex.exe "$artifactDir\codex-xullul-windows-x64.exe" -Force
   $hash = (Get-FileHash "$artifactDir\codex-xullul-windows-x64.exe" -Algorithm SHA256).Hash.ToLowerInvariant()
   Set-Content "$artifactDir\codex-xullul-windows-x64.exe.sha256" "$hash  codex-xullul-windows-x64.exe" -Encoding ASCII
   Copy-Item scripts\install\install-codex-xullul-direct.ps1 "$artifactDir\install-codex-xullul.ps1" -Force
   ```

   The release must include exactly these public assets:

- `codex-xullul-windows-x64.exe`
- `codex-xullul-windows-x64.exe.sha256`
- `install-codex-xullul.ps1`

7. Create release notes.

   Include:

   - the source commit
   - patch notes since the previous release
   - the validation/build notes
   - the SHA-256 hash
   - the exact `irm ... | iex` installer command

8. Tag and publish.

   ```powershell
   git tag -a xullul-v2026.05.09.1 -m "codex-xullul 2026.05.09.1" HEAD
   git push origin xullul-v2026.05.09.1

   gh release create xullul-v2026.05.09.1 `
       --repo xullul/codex `
       --title "codex-xullul 2026.05.09.1" `
       --notes-file target\xullul-release\xullul-v2026.05.09.1\release-notes.md `
       target\xullul-release\xullul-v2026.05.09.1\codex-xullul-windows-x64.exe `
       target\xullul-release\xullul-v2026.05.09.1\codex-xullul-windows-x64.exe.sha256 `
       target\xullul-release\xullul-v2026.05.09.1\install-codex-xullul.ps1
   ```

9. Smoke-test the public installer against an exact release.

   Use a temp install directory so this does not overwrite the normal local
   install while verifying download, checksum, shim creation, and command
   execution:

   ```powershell
   $tempInstall = Join-Path ([System.IO.Path]::GetTempPath()) ("codex-xullul-install-smoke-" + [System.Guid]::NewGuid().ToString("N"))
   $env:CODEX_XULLUL_RELEASE = "2026.05.09.1"
   $env:CODEX_XULLUL_REPO = "xullul/codex"
   $env:CODEX_XULLUL_INSTALL_DIR = $tempInstall
   try {
       irm "https://github.com/xullul/codex/releases/download/xullul-v2026.05.09.1/install-codex-xullul.ps1" | iex
       & (Join-Path $tempInstall "codex-xullul.cmd") --version
   } finally {
       Remove-Item Env:CODEX_XULLUL_RELEASE -ErrorAction SilentlyContinue
       Remove-Item Env:CODEX_XULLUL_REPO -ErrorAction SilentlyContinue
       Remove-Item Env:CODEX_XULLUL_INSTALL_DIR -ErrorAction SilentlyContinue
       Remove-Item -LiteralPath $tempInstall -Recurse -Force -ErrorAction SilentlyContinue
   }
   ```

   The installer updates the user `PATH` when a custom temp install directory is
   used. Remove any temp smoke-test path entry from the user `PATH` after the
   smoke test.

10. Verify final state.

    ```powershell
    gh release view xullul-v2026.05.09.1 --repo xullul/codex --json tagName,name,url,assets
    pwsh -NoLogo -NoProfile -Command "Get-Command codex-xullul | Select-Object -ExpandProperty Source; codex-xullul --version"
    git status --short --branch
    ```

### Install Commands

Install from the latest release:

```powershell
irm https://github.com/xullul/codex/releases/latest/download/install-codex-xullul.ps1 | iex
```

Install an exact release:

```powershell
irm https://github.com/xullul/codex/releases/download/xullul-v2026.05.09.1/install-codex-xullul.ps1 | iex
```

The installer writes:

- `%LOCALAPPDATA%\Programs\Xullul\Codex\bin\codex-xullul.exe`
- `%LOCALAPPDATA%\Programs\Xullul\Codex\bin\codex-xullul.cmd`

If `codex-xullul.exe` is locked because an existing Codex process is running,
the installer falls back to a versioned side-by-side executable and updates the
shim to point at it. It also adds the install directory to the user `PATH`.

### Winget

Winget can install a local manifest directly, which is enough for a personal
package. Microsoft documents local manifest installs with:

```powershell
winget settings --enable LocalManifestFiles
winget install --manifest <path>
```

To create a manifest for a release:

1. Copy `packaging/winget/Xullul.CodexXullul.yaml.template` to a temporary YAML
   file.
2. Replace `VERSION` with the release version, for example `2026.04.28.1`.
3. Replace `SHA256` with the hash from
   `codex-xullul-windows-x64.exe.sha256`.
4. Validate and install it:

```powershell
winget validate .\Xullul.CodexXullul.yaml
winget install --manifest .\Xullul.CodexXullul.yaml
```

If you later want public `winget install Xullul.CodexXullul`, submit the filled
manifest to `microsoft/winget-pkgs`. For private personal use, local manifest
install is lower maintenance.
