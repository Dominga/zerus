// A reply's sentence for a person. hgs answers a refusal as `code: sentence`;
// the code stays in the data (the views and the store tell a refusal from an
// unknown result by it), and a person reads the sentence alone.

/** The sentence without the code the command line puts before it. The words
 * are not changed; only a leading `code: ` is left out. */
export function withoutCode(text: string): string {
  return text.trim().replace(/^[a-z][a-z0-9_]*: /u, "");
}
