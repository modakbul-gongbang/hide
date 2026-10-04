import { INTERFACE_LANGUAGES, type InterfaceLanguage } from "./locale";

/**
 * Resources use i18next's flat keys, {{name}} interpolation and cardinal
 * _one/_other suffixes. This module checks resources; the mature translator
 * owns interpolation, plural selection and React language subscriptions.
 */
export type MessageSchema = Readonly<Record<string, string>>;
export type Catalog<S extends MessageSchema> = { readonly [K in keyof S]: string };
export type Catalogs<S extends MessageSchema> = Readonly<Record<InterfaceLanguage, Catalog<S>>>;

/** Errors identify source keys and language only, never inserted values. */
export class CatalogError extends Error {
  constructor(
    readonly kind: "catalog_languages" | "catalog_keys" | "message_shape" | "message_empty" | "message_placeholders",
    readonly language?: InterfaceLanguage,
    readonly key?: string,
  ) {
    super([kind, language, key].filter((part) => part !== undefined).join(":"));
    this.name = "CatalogError";
  }
}

const PLACEHOLDER = /\{\{([a-zA-Z][a-zA-Z0-9_]*)\}\}/g;

function placeholders(text: string, language: InterfaceLanguage, key: string): string[] {
  const names = [...text.matchAll(PLACEHOLDER)].map((match) => match[1]!);
  if (/\{\{|\}\}/.test(text.replace(PLACEHOLDER, ""))) {
    throw new CatalogError("message_placeholders", language, key);
  }
  return [...new Set(names)].sort();
}

function sameNames(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((name, index) => name === right[index]);
}

/**
 * Check the complete resource set before handing it to the translator.
 * English is the key/schema authority, never a missing-translation fallback.
 * Static Catalogs typing and this check cover typed and untyped resources.
 */
export function validateCatalogs<S extends MessageSchema>(schema: S, catalogs: Catalogs<S>): void {
  if (!sameNames(Object.keys(catalogs).sort(), [...INTERFACE_LANGUAGES].sort())) {
    throw new CatalogError("catalog_languages");
  }
  const keys = Object.keys(schema).sort();
  if (keys.length === 0) throw new CatalogError("catalog_keys");
  for (const language of INTERFACE_LANGUAGES) {
    const catalog = catalogs[language];
    if (!catalog || !sameNames(Object.keys(catalog).sort(), keys)) throw new CatalogError("catalog_keys", language);
    for (const key of keys) {
      const text = catalog[key];
      const reference = schema[key];
      if (typeof reference !== "string" || typeof text !== "string") throw new CatalogError("message_shape", language, key);
      if (!reference.trim() || !text.trim()) throw new CatalogError("message_empty", language, key);
      if (!sameNames(placeholders(reference, "en", key), placeholders(text, language, key))) {
        throw new CatalogError("message_placeholders", language, key);
      }
    }
  }
}
