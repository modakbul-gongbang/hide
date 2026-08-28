# T16 supersession and retirement contract

T16 records the two historical PRDs as superseded by `herdr-ide-rust-native-800mb` without editing sealed PRD or interview files.

The read-only inventory records each historical PRD SHA-256, the current checkout's Electron worktree/tracked-path state, and the standalone Herdr Pet HEAD and tracked-tree digest.

The inventory distinguishes “preserved read-only” from “not present in the current checkout”; neither state authorizes deletion.

Retirement is disabled until a fresh receipt exists, every required V1-V8 row passes, HV1-HV4 review is recorded, exact targets pass dirty-state checks, and the user gives separate cleanup approval.

The exact targets are the two historical PRD paths, any explicitly named Electron reference worktree, the standalone Pet source root, and the installed `Herdr Pet.app` bundle.

No broad process, cache, worktree, app, Pet, or user resource cleanup is performed by T16.

## Read-only command

```sh
./tools/t16-retirement/t16-retirement \
  --project-root /Users/hoyeonlee/projects/herdr-ide \
  --pet-root /Users/hoyeonlee/projects/herdr-pet
PYTHONPATH=tools/t16-retirement python3 -m unittest discover -s tools/t16-retirement/tests -v
```
