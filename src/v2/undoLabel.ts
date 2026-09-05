/** 되돌리기 단추 이름 — 작업 종류별로 «무엇이 어떻게 되는지»를 그대로 적는다.
 *  «되돌리기: 제외한 사진 휴지통으로»처럼 두 동사를 잇지 않는다 (사용자 지적 2026-08-30) */
export function undoLabel(
  kind: string,
  n: number,
  label?: string | null,
): string {
  const c = n.toLocaleString();
  if (kind === "move" && label?.startsWith("폴더 합치기"))
    return `폴더 합치기 되돌리기 (${c}장)`;
  switch (kind) {
    case "trash":
      return `휴지통 보낸 ${c}장 되살리기`;
    case "restore":
      return `되살린 ${c}장 다시 휴지통으로`;
    case "move":
      return `정리 되돌리기 (${c}장)`;
    case "rename":
      return "이름 바꾸기 되돌리기";
    case "import":
      return `가져온 ${c}장 되돌리기`;
    case "capture_date":
      return `촬영일 교정 ${c}장 되돌리기`;
    case "copy":
      return `복사한 ${c}장 지우기`;
    case "publish":
      return `공용 발행 ${c}장 되돌리기`;
    case "folder_create":
      return "만든 폴더 되돌리기";
    case "folder_rename":
      return "폴더 이름 변경 되돌리기";
    case "folder_move":
      return "폴더 이동 되돌리기";
    case "folder_copy":
      return "복사한 폴더 지우기";
    case "folder_trash":
      return "휴지통 보낸 폴더 되살리기";
    case "folder_audit":
      return `폴더 이름 ${c}개 되돌리기`;
    default:
      return `되돌리기 (${c}장)`;
  }
}

const undoable = new Set([
  "move",
  "rename",
  "import",
  "capture_date",
  "copy",
  "publish",
  "folder_create",
  "folder_rename",
  "folder_move",
  "folder_copy",
  "folder_trash",
  "folder_audit",
]);

export const isUndoableBatchKind = (kind: string): boolean =>
  undoable.has(kind);
