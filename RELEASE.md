# Release Process

This document covers the release workflow for the Vespertide ecosystem: the LSP
binary, the VSCode extension, and the Zed extension.

## Release Channels

| Component | Tag pattern | CI workflow | Output |
|---|---|---|---|
| `vespertide-lsp` binary | `lsp-v*` | `.github/workflows/lsp-release.yml` | 5 platform tarballs/zips on GH Release |
| VSCode extension | `vscode-v*` | `.github/workflows/vscode-release.yml` | 5 platform-specific VSIX → Marketplace + Open VSX |
| Zed extension | `zed-v*` | Manual PR to `zed-industries/extensions` | Submodule pin in Zed registry |

## Prerequisites

Repository secrets required (set in GitHub Settings → Secrets):

- `VSCE_PAT` — Visual Studio Marketplace personal access token (Azure DevOps)
- `OVSX_PAT` — Open VSX Registry personal access token (optional; gracefully skipped if absent)
- `GITHUB_TOKEN` — auto-provided by Actions, no setup needed

## Release Flow

### Step 1 — Cut the LSP binary release

```bash
# Bump version in crates/vespertide-lsp/Cargo.toml
# git commit + push
git tag lsp-v0.1.0
git push origin lsp-v0.1.0
```

Triggers `lsp-release.yml`:
- Builds `vespertide-lsp` for 5 platforms (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64)
- Uploads as `vespertide-lsp-{os}-{arch}.{tar.gz,zip}` to GH Release
- Includes SHA256 alongside each archive

Verify on the [Releases page](https://github.com/dev-five-git/vespertide/releases):
- 5 archive assets present (each with a `.sha256` companion)
- Release notes auto-generated from commit log

### Step 2 — Cut the VSCode extension release

After `lsp-v*` Release is live:

```bash
# Bump version in apps/vscode-extension/package.json
# git commit + push
git tag vscode-v0.1.0
git push origin vscode-v0.1.0
```

Triggers `vscode-release.yml`:
- Downloads the corresponding `vespertide-lsp-*` asset from the latest `lsp-v*` Release
- Extracts into `apps/vscode-extension/bin/<platform>/`
- Builds via `bun + esbuild`
- Packages 5 platform-specific `.vsix` files via `vsce`
- Publishes to VS Code Marketplace (via `VSCE_PAT`)
- Publishes to Open VSX (via `OVSX_PAT`, non-fatal)
- Uploads `.vsix` files to the `vscode-v*` GH Release for manual install fallback

Pinning a specific LSP tag (rather than latest):

```bash
gh workflow run vscode-release.yml \
  --field lsp_tag=lsp-v0.1.0 \
  --field vscode_tag=vscode-v0.1.0
```

### Step 3 — Cut the Zed extension release

Zed publishes via PR to a community-maintained registry, not via automated workflow.

#### 3a. Bump and tag

```bash
# Bump version in apps/zed-extension/extension.toml and Cargo.toml
# git commit + push
git tag zed-v0.1.0
git push origin zed-v0.1.0
```

The `zed-v*` tag itself does not trigger any workflow. It serves as a reference
point for the Zed registry submission.

#### 3b. Submit PR to zed-industries/extensions

1. Fork `https://github.com/zed-industries/extensions` (one-time)
2. Clone your fork
3. Add or update the Vespertide submodule:

```bash
# First time:
git submodule add https://github.com/dev-five-git/vespertide.git \
  extensions/vespertide

# Subsequent releases (advance the submodule SHA):
cd extensions/vespertide
git fetch origin zed-v0.1.0
git checkout zed-v0.1.0
cd ../..
```

Note: the Zed registry expects the submodule root to contain
`apps/zed-extension/`. Some maintainers use a dedicated `zed-vespertide` repo
solely containing the extension. If `zed-industries/extensions` rejects the
nested layout, see [Layout fallback](#layout-fallback) below.

4. Edit `extensions.toml` in the registry root:

```toml
[vespertide]
submodule = "extensions/vespertide"
version = "0.1.0"
path = "apps/zed-extension"   # path inside the submodule, if supported
```

5. Sort + commit:

```bash
pnpm sort-extensions
git add extensions/vespertide extensions.toml
git commit -m "vespertide: add v0.1.0"
git push
```

6. Open PR. The Zed CI will:
   - Validate the WIT manifest
   - Build the extension to WASM
   - Verify the LICENSE file
   - Confirm `id` uniqueness

7. Once merged, the extension is auto-published. Users install via
   `zed: extensions` → search "Vespertide" → Install.

#### Layout fallback

If `zed-industries/extensions` rejects the nested `apps/zed-extension/` path,
mirror the extension to a dedicated repository:

```bash
# Create a new public repo, e.g. dev-five-git/zed-vespertide
git subtree split --prefix=apps/zed-extension -b zed-extension-mirror
git push https://github.com/dev-five-git/zed-vespertide.git \
  zed-extension-mirror:main --force
```

Then submit `dev-five-git/zed-vespertide` as the submodule instead.

## Rollback

### Rollback an LSP release

```bash
# Delete the bad release + tag
gh release delete lsp-v0.1.0 --yes
git push --delete origin lsp-v0.1.0
git tag -d lsp-v0.1.0

# Cut a corrected release
git tag lsp-v0.1.1
git push origin lsp-v0.1.1
```

VSCode/Zed users on the bad version will not auto-revert. Tell them to update.

### Rollback a VSCode release

```bash
# Mark as deprecated on the Marketplace
bunx vsce unpublish dev-five-git.vespertide@0.1.0

# Or, if Marketplace already has a newer version, push a hotfix:
git tag vscode-v0.1.1
git push origin vscode-v0.1.1
```

Open VSX is harder to unpublish — contact the maintainers.

### Rollback a Zed release

Zed extensions cannot be unpublished. Open a follow-up PR to the registry
that bumps the version with a fix or yanks the entry by setting it to a
known-good prior version.

## Pre-release vs Stable Versioning

VSCode convention: **odd minor versions are pre-release**, **even minor versions
are stable**.

- `0.1.x` — pre-release (use `vsce publish --pre-release`)
- `0.2.x` — first stable
- `0.3.x` — pre-release again
- `0.4.x` — stable

To cut a pre-release:

```bash
# Tag with -pre suffix to differentiate
git tag vscode-v0.1.0-pre1
git push origin vscode-v0.1.0-pre1
```

Adjust `vscode-release.yml` to forward `--pre-release` to `vsce publish` if the
tag contains `-pre`.

## Verification Checklist (per release)

- [ ] All 5 LSP binaries present on `lsp-v*` GH Release
- [ ] Each binary has its `.sha256` companion file
- [ ] VSCode Marketplace shows the new version (search "vespertide")
- [ ] Open VSX shows the new version (https://open-vsx.org/extension/dev-five-git/vespertide)
- [ ] Zed registry PR merged + extension installable from `zed: extensions`
- [ ] README + CHANGELOG updated to reference the new version
- [ ] No regressions in `cargo test --workspace` on `refactor`/`main`

## Related Documentation

- [`apps/vscode-extension/README.md`](apps/vscode-extension/README.md) — extension-specific docs
- [`apps/zed-extension/README.md`](apps/zed-extension/README.md) — Zed-specific docs
- [`docs/PERFORMANCE-AUDIT.md`](docs/PERFORMANCE-AUDIT.md) — perf history
- [`docs/PARALLELIZATION.md`](docs/PARALLELIZATION.md) — concurrency design
