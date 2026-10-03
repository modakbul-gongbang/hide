# Neutral keyboard focus treatment

Status: approved by the user on 2026-10-03 ("b안 괜찮은듯?").

The active Agent and View areas currently draw a primary-colored content perimeter in addition to their tab indicator.
Replace that treatment with the selected B proposal from the focus-indicator-alternatives scratch.

## Decisions

| ID | Approved behavior |
| --- | --- |
| D-01 | Remove the visible area perimeter in both Agent and View columns. |
| D-02 | Use the existing foreground token for the keyboard area's selected-tab underline; retain the existing tab geometry and header wash. |
| D-03 | Only a split pane with terminal keyboard focus carries a one-pixel subtle-foreground outline, including while its menu is open. |
| D-04 | A lone or zoomed pane has no outline; moving focus to a View, sidebar or another application hides the split outline. |
| D-05 | Retain inactive selections and readable content, with no blur, dimming, new token or focus-state logic. |

The approved comparison remains local in agents/runs/focus-indicator-alternatives/design in the root checkout.
The area-focus reference bundle is frozen before implementation under agents/runs/focus-treatment/design/baseline/approved-b.
Update the owning behavior guide, Pen library and generated Workspace screens with the source change.
Verify Light and Dark, focus transfer, split and zoom behavior, constant area geometry, and the actual isolated desktop window.
Installing the candidate over the operator's application is outside this change.
