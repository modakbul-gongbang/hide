# Security policy

## Reporting a vulnerability

Report it privately through GitHub's private vulnerability reporting for this repository: open the **Security** tab and choose **Report a vulnerability**.
Do not open a public issue.

You will get an acknowledgement within a week.
Fixes ship as a normal release; the advisory is published once the release is out.

## What is in scope

- The hide app: the Swift shell under `macos/` and the Rust core under `herdr-core/`.
- The build and release scripts under `scripts/` and the workflows under `.github/workflows/`.
- The bundled Herdr runtime pin. hide ships a specific Herdr binary and verifies its digest at build time; a problem in Herdr itself belongs to [Herdr](https://herdr.dev), but a problem in how hide pins, verifies, or launches it belongs here.

## What hide does not do

hide never asks for credentials.
Remote machines are reached through the operator's existing SSH configuration, and the app stores no secret of its own.
A report that finds it doing otherwise is in scope.

## Supported versions

Only the latest release receives fixes.
