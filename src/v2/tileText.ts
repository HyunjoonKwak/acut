import { fmtBytes, fmtDate } from "./format";
import type { Prefs } from "./prefs";
import type { TileFile } from "./Tile";

/** 설정 «타일 배지»에 따라 왼쪽 아래에 적을 글. 없으면 null. */
export function badgeText(
  file: TileFile,
  badge: Prefs["badge"],
): string | null {
  switch (badge) {
    case "iso":
      return file.iso ? `ISO ${file.iso}` : null;
    case "shutter":
      return file.shutter ?? null;
    case "aperture":
      return file.aperture ? `f${file.aperture}` : null;
    case "focal":
      return file.focal_mm ? `${file.focal_mm}mm` : null;
    default:
      return null;
  }
}

/** 이름줄 한 줄의 글 */
export function captionText(
  file: TileFile,
  what: Prefs["caption1"] | Prefs["caption2"],
): string {
  switch (what) {
    case "name":
      return file.name;
    case "date":
      return fmtDate(file.taken_at);
    case "size":
      return fmtBytes(file.size);
    case "camera":
      return file.cam_model ?? "";
    case "dateSize":
      return `${fmtDate(file.taken_at)} · ${fmtBytes(file.size)}`;
    default:
      return "";
  }
}
