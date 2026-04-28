## Xullul Release

This fork publishes personal Windows builds as `codex-xullul`. The release path
is intentionally small: GitHub Release hosts the executable, the custom
PowerShell installer downloads that executable, and the Winget manifest points
at the same asset.

### GitHub Release

Run the **xullul-release** GitHub Actions workflow manually. It builds:

- `codex-xullul-windows-x64.exe`
- `codex-xullul-windows-x64.exe.sha256`
- `install-codex-xullul.ps1`

If you leave the workflow `version` input blank, it uses
`yyyy.MM.dd.<run-number>` and creates a tag named `xullul-v<version>`.

Install from the latest release:

```powershell
irm https://github.com/xullul/codex/releases/latest/download/install-codex-xullul.ps1 | iex
```

Install an exact release:

```powershell
$env:CODEX_XULLUL_RELEASE = "2026.04.28.1"
irm https://github.com/xullul/codex/releases/download/xullul-v2026.04.28.1/install-codex-xullul.ps1 | iex
```

The installer writes:

- `%LOCALAPPDATA%\Programs\Xullul\Codex\bin\codex-xullul.exe`
- `%LOCALAPPDATA%\Programs\Xullul\Codex\bin\codex-xullul.cmd`

It also adds that directory to the user `PATH`.

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
