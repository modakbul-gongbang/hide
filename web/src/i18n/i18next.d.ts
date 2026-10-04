import "i18next";
import type { english } from "./catalogs";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    keySeparator: false;
    nsSeparator: false;
    returnNull: false;
    returnObjects: false;
    strictKeyChecks: true;
    resources: { translation: typeof english };
  }
}
