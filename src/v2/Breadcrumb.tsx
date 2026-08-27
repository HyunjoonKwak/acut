/**
 * 지금 어디를 보고 있는지 — 툴바 왼쪽.
 *
 * 사이드바에서 고른 것이 무엇인지 화면에도 있어야 한다. 사이드바를 접으면
 * 아무 단서가 없어 "이게 전체인가 폴더인가"를 알 수 없었다.
 */
export default function Breadcrumb({
  libs,
  libId,
  folder,
  viewTrash,
  matched,
}: {
  libs: { id: number; name: string; online: boolean }[];
  libId: number | null;
  /** 라이브러리 기준 폴더 경로. null이면 라이브러리 전체 */
  folder: string | null;
  viewTrash: boolean;
  /** 지금 조건에 걸린 장수 */
  matched: number;
}) {
  const lib = libs.find((l) => l.id === libId);

  const parts: string[] = viewTrash
    ? ["휴지통"]
    : [lib ? lib.name : "전체", ...(folder ? folder.split("/") : [])];

  return (
    <div className="flex items-baseline gap-2 min-w-0">
      <div className="flex items-baseline gap-1.5 min-w-0">
        {parts.map((p, i) => (
          <span key={i} className="flex items-baseline gap-1.5 min-w-0">
            {i > 0 && <span className="text-fg-faint text-[11px]">›</span>}
            <span
              className={`truncate ${
                i === parts.length - 1
                  ? "text-fg text-[14px] font-semibold"
                  : "text-fg-mute text-[12.5px]"
              }`}
              title={p}
            >
              {p}
            </span>
          </span>
        ))}
      </div>
      {matched > 0 && (
        <span className="text-fg-mute text-[12px] tabular-nums shrink-0">
          {matched.toLocaleString()}장
        </span>
      )}
      {lib && !lib.online && (
        <span className="text-drop text-[11px] shrink-0">연결 안 됨</span>
      )}
    </div>
  );
}
