# T9 brand guidelines and asset decision

Date: 2026-08-30.

## Decision

The shipped build keeps product-owned `C` and `O` text glyphs for Claude and Codex instead of bundling either provider's official logo.

The glyphs are rendered by the existing agent rows and agent picker in `macos/Sources/HerdrMacOS/HideUI.swift`.

The resource inventory contains the product-owned `hide.icns` and `hide-icon-1024.png`, but no Claude, Codex, Anthropic, or OpenAI logo file.

The app does not claim partnership, sponsorship, or endorsement by either provider.

This is the T9 fallback path required when a provider's public guidance does not clearly grant the intended redistribution and in-product branding use.

## Official sources checked

| Provider | Official source | Result used for the decision |
| --- | --- | --- |
| Anthropic | [Anthropic Newsroom](https://www.anthropic.com/news) | The official newsroom links its media-assets entry to the Anthropic press kit. |
| Anthropic | [Anthropic official Brandfolder](https://brandfolder.com/anthropic/) | The public page identifies Brandfolder as Anthropic's source for official brand assets and instructs users to follow the usage guidelines. |
| Anthropic | [Anthropic Consumer Terms](https://www.anthropic.com/legal/consumer-terms) | The terms reserve Anthropic's intellectual-property rights and require compliance with posted guidelines and supplemental terms. |
| OpenAI | [OpenAI Design Guidelines](https://openai.com/brand/) | The page says marks belong to OpenAI, limits logo use to contexts directly related to OpenAI services, requires the supplied form and acknowledgement, and prohibits unapproved or modified use and incorporation into another brand. |
| OpenAI | [OpenAI full design guidelines](https://brand.openai.com/) | This is the full-guideline destination linked by OpenAI's public design page. |

## Findings

Anthropic's official newsroom and Brandfolder source were confirmed, but no public text page spelling out a permission for redistributing a Claude mark inside an unrelated product bundle was found in the checked official sources.

OpenAI's public guidance is explicit enough to reject the planned use of an OpenAI-derived `O` as an app-owned logo because it prohibits incorporating a mark into another brand or using an unapproved variation.

The second finding is a conservative interpretation of the official wording, not a legal opinion.

No provider logo was copied into `macos/Resources`, no provider logo is referenced by the package manifest, and the current fallback glyphs remain visibly distinct from official logo artwork.

The product icon is separately owned by hide and was generated from the selected candidate `docs/assets/hide-icon-candidates/hide-icon-02.png` by `macos/scripts/generate_app_icon.sh`.

The bundled Herdr notice remains at `macos/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt` and records the Apache-2.0 source, URL, version, and SHA-256.

## AC20 mapping

AC20 evidence is supplied by the bundled Herdr Apache-2.0 notice, this dated source record, and the explicit provider-logo fallback decision.

The remaining acceptance judgment is still owned by the Sasu verification gate because AC20 is a judged criterion.
