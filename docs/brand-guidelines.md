# App icon and bundled marks

This is a resource-ownership guide, not a claim of trademark approval or a second UI design specification.
Use [DESIGN.md](../DESIGN.md#in-product-components) for native shell appearance.

## App identity

The release product display name is `hide` and its bundle identifier is `me.grab.hide`.
The dev bundle script gives linked worktrees their own name and identifier; see [dev-runtime.md](dev-runtime.md).

The build bundles [hide.icns](../macos/Resources/hide.icns).
[generate_app_icon.sh](../macos/scripts/generate_app_icon.sh) defaults to [hide-icon-02.png](assets/hide-icon-candidates/hide-icon-02.png) as its source and produces the prepared PNG and ICNS.
The other candidates are reference artwork, not alternate runtime choices or proof of UI approval.

## Third-party resources

Provider artwork is bundled as `agent-claude.png` and `agent-codex.png` under `macos/Sources/HerdrMacOS/Resources/`.
The earlier claim that the picker used only original single-letter glyphs is obsolete.

Keep attribution and packaging aligned with [THIRD_PARTY_NOTICES](../macos/Resources/THIRD_PARTY_NOTICES/).
Before replacing provider artwork or making new public brand claims, review the provider's current official guidance; this document does not grant permission or preserve an old legal verdict as current.
