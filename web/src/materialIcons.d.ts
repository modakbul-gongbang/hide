// The part of material-icon-theme's manifest fileIcons.ts reads, typed here
// rather than through resolveJsonModule, which would make the compiler infer a
// literal type for every one of its thousands of names.
declare module "material-icon-theme/dist/material-icons.json" {
  type Names = Record<string, string>;
  export const iconDefinitions: Record<string, { iconPath: string }>;
  export const fileNames: Names;
  export const fileExtensions: Names;
  export const folderNames: Names;
  export const folderNamesExpanded: Names;
  export const light: { fileNames?: Names; fileExtensions?: Names; folderNames?: Names; folderNamesExpanded?: Names };
  export const file: string;
  export const folder: string;
  export const folderExpanded: string;
}
