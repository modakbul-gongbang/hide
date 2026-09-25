// Every System part and the states its `System / <Name>` sheet in
// design/hide-ui.lib.pen draws, by the same names. The gallery types each
// section's renderers from this list, so TypeScript refuses a state the page
// does not render, and scripts/check-pen-gallery.mjs refuses a list that is
// not the Pen library's. Keep it a plain literal: the check reads it as JSON.

export const GALLERY = {
  "Button": ["Default", "Default Hover", "Default Focus", "Default Disabled", "Pending", "Secondary", "Secondary Hover", "Outline", "Ghost", "Ghost Hover", "Destructive", "Destructive Hover", "Link", "Small", "Large", "Icon", "Icon Small"],
  "Input": ["Default", "Placeholder", "Filled", "Focus", "Invalid", "Disabled", "Mono"],
  "Select": ["Default", "Placeholder", "Focus", "Disabled", "Open"],
  "Checkbox": ["Unchecked", "Checked", "Focus", "Disabled", "Checked Disabled"],
  "Switch": ["Off", "On", "Focus", "Disabled"],
  "Radio Group": ["Default", "Focus", "Disabled"],
  "Toggle Group": ["Default", "Hover", "Focus", "Disabled"],
  "Slider": ["Default", "Focus", "Disabled"],
  "Tabs": ["Default", "Hover", "Focus", "Disabled"],
  "Badge": ["Default", "Secondary", "Destructive", "Outline"],
  "Kbd": ["Default", "Group"],
  "Separator": ["Horizontal", "Vertical"],
  "Dropdown Menu": ["Closed", "Open", "Highlighted", "Disabled Item", "Destructive Item"],
  "Context Menu": ["Open", "Highlighted", "Disabled Item"],
  "Dialog": ["Open", "With Close Button"],
  "Alert Dialog": ["Open", "Pending"],
  "Sheet": ["Right", "Left"],
  "Popover": ["Open"],
  "Tooltip": ["Open", "With Shortcut"],
  "Command": ["Default", "Filtered", "Empty"],
  "Sonner": ["Default", "With Description", "With Action"]
} as const;

export type Section = keyof typeof GALLERY;
export type StateOf<S extends Section> = (typeof GALLERY)[S][number];
