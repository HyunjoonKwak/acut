import { fmtBytes, fmtDateTime, fmtDuration, megapixels } from "./format";

export type FocusInfo = {
  name: string;
  size: number;
  kind: number;
  width: number | null;
  height: number | null;
  taken_at: number;
  duration_ms: number | null;
};

function Cell({ icon, children }: { icon: string; children: React.ReactNode }) {
  return (
    <span className="flex items-center gap-1 shrink-0 min-w-0">
      <span className="text-fg-faint">{icon}</span>
      <span className="truncate">{children}</span>
    </span>
  );
}

/**
 * 하단 상태바 — **지금 보고 있는 사진**의 정보.
 *
 * Lap의 StatusBar와 같은 구성이다. 왼쪽부터 순서대로:
 *   몇 번째/전체 · 파일명(크기) · 해상도 · 촬영일시 · 카메라 · 촬영 설정
 *
 * 라이브러리 합계만 띄우던 자리였다. 사진을 훑을 때 알고 싶은 건 합계가
 * 아니라 **지금 이 사진**이 무엇이냐다. 인스펙터를 열지 않고도 보이게 한다.
 */
export default function StatusBar({
  index,
  total,
  totalBytes,
  file,
  exif,
  children,
}: {
  /** 지금 고른 사진이 전체에서 몇 번째 (0부터). 없으면 -1 */
  index: number;
  total: number;
  totalBytes: number;
  file: FocusInfo | null;
  /** 카메라·설정. 상세를 아직 못 읽었으면 null */
  exif: {
    camModel: string | null;
    lens: string | null;
    settings: string;
  } | null;
  /** 오른쪽에 붙는 것 — 진행·되돌리기 */
  children?: React.ReactNode;
}) {
  return (
    <div className="h-8 shrink-0 flex items-center gap-4 px-3 bg-chrome border-t border-line text-[11.5px] text-fg-mute tabular-nums">
      <Cell icon="≡">
        {index >= 0 && (
          <span className="text-fg-dim">{(index + 1).toLocaleString()}/</span>
        )}
        {total.toLocaleString()}장 · {fmtBytes(totalBytes)}
      </Cell>

      {file && (
        <>
          <Cell icon={file.kind === 1 ? "▶" : "▣"}>
            <span className="text-fg-dim">{file.name}</span> (
            {fmtBytes(file.size)})
          </Cell>
          {file.width && file.height && (
            <Cell icon="⤢">
              {file.width} × {file.height}
              {megapixels(file.width, file.height) && (
                <span className="text-fg-faint">
                  {" "}
                  {megapixels(file.width, file.height)}
                </span>
              )}
            </Cell>
          )}
          <Cell icon="🗓">{fmtDateTime(file.taken_at)}</Cell>
          {file.duration_ms ? (
            <Cell icon="⏱">{fmtDuration(file.duration_ms)}</Cell>
          ) : null}
          {exif?.camModel && (
            <Cell icon="📷">
              {exif.camModel}
              {exif.lens && (
                <span className="text-fg-faint"> ({exif.lens})</span>
              )}
            </Cell>
          )}
          {exif?.settings && <Cell icon="◎">{exif.settings}</Cell>}
        </>
      )}

      <div className="flex-1" />
      {children}
    </div>
  );
}
