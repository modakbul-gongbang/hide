---
id: INV-pet-state-off-main-thread
kind: invariant
status: active
evidence:
  - "session 2a22f53f (2026-08-18): sync get_pet_state read agent files on the main thread every 1000ms and caused periodic hitches during fast drags"
trigger:
  paths:
    - "apps/pet-app/src-tauri/src/main.rs"
check:
  type: grep
  pattern: "async fn get_pet_state"
  files: "apps/pet-app/src-tauri/src/main.rs"
  present: true
---

`get_pet_state` reads agent files off disk. A sync Tauri command runs on the
main thread, where the 8ms native drag loop also runs, so this command must
stay `async` (thread-pool execution). Reverting it to a sync fn reintroduces
periodic drag hitches.
