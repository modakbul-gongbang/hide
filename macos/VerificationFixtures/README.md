# Herdr IDE verification fixture

This synthetic workspace validates the three-column shell without touching a user workspace.

- Sidebar state comes from an in-process fixture.
- Terminal bytes cross the real Rust C ABI and SwiftTerm boundary.
- Workbench reads existing local files only.
