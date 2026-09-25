import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// The token scales are this shell's own names (design/tokens.json), so the
// merger is told which `text-*` is a size and which `p-*` is spacing; without
// it `text-body` would read as a color and drop `text-foreground`.
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: ["micro", "caption", "body", "subhead", "title", "headline"],
      spacing: ["none", "xxs", "xs", "sm", "md", "lg", "xl", "xxl", "xxxl"],
      radius: ["xs", "sm", "md", "lg", "xl"],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
