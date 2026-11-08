// Task 245: the tests read numbers and dates in en-US, whatever the machine's
// locale (a Russian Windows formats 8193 as "8 193" and 7.0 as "7,0").
// Production code keeps the default locale; only calls that name none are
// pinned here, in the test run.
const LOCALE = "en-US";
type Loose = (...args: unknown[]) => unknown;
const pick = (locales: unknown) => locales ?? LOCALE;

/** A constructor that takes `LOCALE` when it is given no locale; `instanceof` still holds. */
function pinned(Base: new (...args: unknown[]) => object) {
  const Pinned = function (locales?: unknown, options?: unknown) { return new Base(pick(locales), options); };
  Object.setPrototypeOf(Pinned, Base);
  Pinned.prototype = Base.prototype;
  return Pinned;
}
for (const name of ["NumberFormat", "DateTimeFormat", "PluralRules", "Collator"] as const) {
  Object.defineProperty(Intl, name, { value: pinned(Intl[name] as unknown as new (...args: unknown[]) => object), writable: true, configurable: true });
}

/** A prototype method whose locale argument (at `at`) defaults to `LOCALE`. */
function pinMethod(target: object, name: string, at: number) {
  const original = (target as Record<string, Loose>)[name];
  Object.defineProperty(target, name, {
    value: function (this: unknown, ...args: unknown[]) {
      args[at] = pick(args[at]);
      return original.apply(this, args);
    },
    writable: true,
    configurable: true,
  });
}
pinMethod(Number.prototype, "toLocaleString", 0);
pinMethod(BigInt.prototype, "toLocaleString", 0);
pinMethod(Date.prototype, "toLocaleString", 0);
pinMethod(Date.prototype, "toLocaleDateString", 0);
pinMethod(Date.prototype, "toLocaleTimeString", 0);
pinMethod(String.prototype, "localeCompare", 1);

export {};
