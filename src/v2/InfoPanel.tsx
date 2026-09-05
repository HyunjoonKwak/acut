import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Histogram from "./Histogram";
import CommentBox from "./CommentBox";
import TagEditor from "./TagEditor";
import { CameraRows, DetailRows, Row, Sep } from "./detail";
import { verdictOf, type Detail } from "./detailText";
import { thumbUrl, type FileRow } from "./types";

/**
 * 정보 패널 — 격자 옆에 붙어 지금 고른 한 장의 상세를 보인다 (Lap의 «정보 보기»).
 *
 * 뷰어를 열지 않아도 촬영·카메라·히스토그램·태그·메모를 본다. 히스토그램은
 * 썸네일로 그린다 — 원본을 매번 풀면 한 장 옮길 때마다 수백 ms가 걸린다.
 */
export default function InfoPanel({
  file,
  onClose,
}: {
  file: FileRow | null;
  onClose: () => void;
}) {
  // 상세는 반드시 어느 사진 것인지와 함께 둔다. A를 읽는 중 B로 옮겼을 때
  // A의 메모가 B의 CommentBox 초기값이 되면 B에 그대로 덮어쓸 수 있다.
  const [got, setGot] = useState<{
    id: number;
    detail: Detail | null;
    error: string | null;
  } | null>(null);
  const id = file?.id ?? null;
  const detail = id !== null && got?.id === id ? got.detail : null;
  const error = id !== null && got?.id === id ? got.error : null;

  useEffect(() => {
    let live = true;
    if (id === null) {
      queueMicrotask(() => live && setGot(null));
      return () => {
        live = false;
      };
    }
    invoke<Detail>("file_detail", { id })
      .then((d) => live && setGot({ id, detail: d, error: null }))
      .catch((e) => live && setGot({ id, detail: null, error: String(e) }));
    return () => {
      live = false;
    };
  }, [id]);

  const thumb = file ? thumbUrl(file) : null;

  return (
    <aside className="w-64 shrink-0 bg-raised border-l border-line overflow-y-auto text-[13px]">
      <div className="flex items-center justify-between px-4 pt-3 pb-1">
        <span className="text-[11.5px] text-fg-mute uppercase tracking-wider">
          정보
        </span>
        <button
          onClick={onClose}
          aria-label="정보 패널 닫기"
          title="정보 패널 닫기 (I)"
          className="w-6 h-6 rounded text-fg-mute hover:text-fg hover:bg-canvas"
        >
          ×
        </button>
      </div>

      {!file || !detail ? (
        <div className="px-4 py-6 text-fg-faint text-[12.5px]">
          {error
            ? `상세 정보를 읽지 못했습니다 — ${error}`
            : file
              ? "읽는 중…"
              : "사진을 고르면 여기에 상세가 뜹니다"}
        </div>
      ) : (
        <div className="px-4 pb-4">
          {thumb && (
            <img
              src={thumb}
              alt=""
              className="w-full rounded mb-3 object-contain bg-canvas"
              style={{ maxHeight: 160 }}
            />
          )}
          <div className="text-fg break-all mb-2" title={detail.name}>
            {detail.name}
          </div>
          <DetailRows detail={detail} />
          <Row k="판정" v={verdictOf(detail)} />
          <CommentBox
            key={file.id}
            id={file.id}
            initial={detail.comment ?? ""}
            onSaved={(c) =>
              setGot((d) =>
                d?.id === file.id && d.detail
                  ? { ...d, detail: { ...d.detail, comment: c } }
                  : d,
              )
            }
          />
          <CameraRows detail={detail} />

          {/* 영상은 대표 프레임 한 장이라 분포가 큰 뜻이 없다 */}
          {file.kind !== 1 && thumb && (
            <>
              <Sep />
              <Histogram src={thumb} />
            </>
          )}

          <Sep />
          <TagEditor id={file.id} />
        </div>
      )}
    </aside>
  );
}
