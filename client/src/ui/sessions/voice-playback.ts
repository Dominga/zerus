/** One exact native preview, as raw bytes, never JSON. Retain at most one
 * bounded Blob for WebKit's media backend; it refuses custom URI schemes. */
export function recordingBlob(bytes: ArrayBuffer, video: boolean): Blob {
  if (!bytes.byteLength || bytes.byteLength > 10 * 1024 * 1024) throw new Error("Could not read the saved recording.");
  return new Blob([bytes], { type: video ? "video/mp4" : "audio/wav" });
}
