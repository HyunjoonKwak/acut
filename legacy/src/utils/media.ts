import { convertFileSrc } from "@tauri-apps/api/core";

// Thumbnails are JPEG files in the app data dir (path stored in DB).
// Old databases may still carry inline base64 until Phase 1 re-runs.
// Discriminator: cache paths always end in ".jpg" while the base64
// alphabet contains no "." (note base64 JPEG starts with "/9j/", so a
// leading-slash check would misroute it).
export function thumbSrc(thumbnail: string | null | undefined): string | null {
  if (!thumbnail) return null;
  return thumbnail.endsWith(".jpg")
    ? convertFileSrc(thumbnail)
    : `data:image/jpeg;base64,${thumbnail}`;
}
