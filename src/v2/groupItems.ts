/**
 * 묶기 갈래 — 값과 이름만. 그림은 GroupMenu.tsx.
 *
 * 순수 .ts에 두는 이유: 설정 스토어·목록 훅·테스트가 이 타입을 쓰는데,
 * .tsx에서 가져오면 JSX 없는 환경(node 테스트, tsconfig.node)에서 못 읽는다.
 */
/** 백엔드 `db::query::GroupBy`와 이름이 같아야 한다 */
export type GroupBy =
  | "none"
  | "folder"
  | "day"
  | "month"
  | "year"
  | "rating"
  | "camera"
  | "lens"
  | "file_type"
  | "culling";

export const GROUP_ITEMS: { by: GroupBy; label: string }[] = [
  { by: "none", label: "묶지 않음" },
  { by: "day", label: "날짜" },
  { by: "month", label: "월" },
  { by: "year", label: "연도" },
  { by: "folder", label: "폴더" },
  { by: "rating", label: "평점" },
  { by: "culling", label: "판정" },
  { by: "file_type", label: "종류" },
  { by: "camera", label: "카메라" },
  { by: "lens", label: "렌즈" },
];
