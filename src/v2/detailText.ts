/** `file_detail`이 주는 한 장의 상세 — 글자 조각. 컴포넌트는 detail.tsx 에 있다
 *  (한 파일에 섞으면 핫리로드가 깨진다). */
export type Detail = {
  name: string;
  folder: string;
  size: number;
  takenAt: number;
  takenAtSource: number;
  width: number | null;
  height: number | null;
  camMake: string | null;
  camModel: string | null;
  lens: string | null;
  iso: number | null;
  aperture: number | null;
  shutter: string | null;
  focalMm: number | null;
  durationMs: number | null;
  rating: number;
  cullingFlag: number;
  favorite: boolean;
  kind: number;
  comment: string | null;
};

export const SOURCE_LABEL = [
  "EXIF",
  "파일명 추정",
  "파일시각 추정",
  "알 수 없음",
];

/** 셔터·조리개·ISO·초점거리를 한 줄로 */
export function settingsOf(d: Detail): string {
  return (
    [
      d.shutter,
      d.aperture ? `f${d.aperture}` : null,
      d.iso ? `ISO ${d.iso}` : null,
      d.focalMm ? `${d.focalMm}mm` : null,
    ]
      .filter(Boolean)
      .join(" · ") || "—"
  );
}

/** 별점·남김/제외·즐겨찾기를 한 줄로. 아무것도 없으면 «—» */
export function verdictOf(d: Detail): string {
  return (
    [
      d.rating > 0 ? "★".repeat(d.rating) : null,
      d.cullingFlag === 1 ? "남김" : d.cullingFlag === 2 ? "제외" : null,
      d.favorite ? "♥" : null,
    ]
      .filter(Boolean)
      .join(" · ") || "—"
  );
}
