import { fmtBytes, fmtDateTime, fmtDuration, megapixels } from "./format";
import { SOURCE_LABEL, settingsOf, type Detail } from "./detailText";

/** 상세를 그리는 조각 — 뷰어 인스펙터와 격자 옆 정보 패널이 같이 쓴다 */
export function Row({ k, v, dim }: { k: string; v: string; dim?: boolean }) {
  return (
    <div className="flex justify-between gap-3 py-[3px]">
      <span className="text-fg-mute shrink-0">{k}</span>
      <span
        className={`text-right truncate font-mono text-[12px] ${dim ? "text-keep" : "text-fg-dim"}`}
        title={v}
      >
        {v}
      </span>
    </div>
  );
}

export function Sep() {
  return <div className="h-px bg-line my-3" />;
}

/** 촬영·크기·용량·폴더 */
export function DetailRows({ detail }: { detail: Detail }) {
  const mp =
    detail.width && detail.height
      ? megapixels(detail.width, detail.height)
      : "";
  return (
    <>
      <Row k="촬영" v={fmtDateTime(detail.takenAt)} />
      <Row
        k="날짜 출처"
        v={SOURCE_LABEL[detail.takenAtSource] ?? "?"}
        dim={detail.takenAtSource !== 0}
      />
      <Row
        k="크기"
        v={
          detail.width && detail.height
            ? `${detail.width} × ${detail.height}${mp ? ` · ${mp}` : ""}`
            : "—"
        }
      />
      <Row k="용량" v={fmtBytes(detail.size)} />
      {detail.durationMs ? (
        <Row k="길이" v={fmtDuration(detail.durationMs)} />
      ) : null}
      <Row k="폴더" v={detail.folder || "/"} />
    </>
  );
}

/** 카메라·렌즈·설정 — 카메라 정보가 하나도 없으면 아무것도 그리지 않는다 */
export function CameraRows({ detail }: { detail: Detail }) {
  if (!detail.camModel && !detail.lens) return null;
  return (
    <>
      <Sep />
      <Row k="카메라" v={detail.camModel ?? "—"} />
      <Row k="렌즈" v={detail.lens ?? "—"} />
      <Row k="설정" v={settingsOf(detail)} />
    </>
  );
}
