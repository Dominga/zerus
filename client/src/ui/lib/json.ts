// Reading JSON from another computer: a field of the wrong type reads as
// empty, never as a thrown error (QJsonValue's toObject, toArray, toString).

export type Json = Record<string, unknown>;

/** An object, else `{}`. */
export const object = (value: unknown): Json => (value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Json) : {});

/** An array, else `[]`. */
export const array = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

/** A string, else "". */
export const string = (value: unknown): string => (typeof value === "string" ? value : "");
