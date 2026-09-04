import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { areaLabel } from "./areaItems";
import { fmtBytes } from "./format";
import { toast } from "./toastStore";
import { Btn } from "./ui";

type Size = { folders: number; files: number; bytes: number };

/**
 * 다른 디스크로 옮기기 — 폴더 한 갈래를 통째로 다른 라이브러리로.
 *
 * 운영 SSD가 차면 오래된 연도를 아카이브 디스크로. 옮긴 뒤에도 목록에 그대로
 * 보이고, 디스크를 빼면 흐려질 뿐이다. 되돌리기는 같은 동작으로 원래
 * 라이브러리를 고르면 된다.
 */
export default function OffloadDialog({
  folderId,
  name,
  libraryId,
  onClose,
}: {
  folderId: number;
  name: string;
  /** 지금 든 라이브러리 */
  libraryId: number;
  onClose: () => void;
}) {
  const libs = useData((s) => s.libs);
  const [size, setSize] = useState<Size | null>(null);
  const [dest, setDest] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const targets = libs.filter((l) => l.id !== libraryId);
  // 내사진·공용은 Drive가 보는 폴더 — 여기서 옮기면 NAS에서도 지워진다
  const synced = [1, 2].includes(
    libs.find((l) => l.id === libraryId)?.area ?? 3,
  );

  useEffect(() => {
    invoke<Size>("folder_size", { folderId })
      .then(setSize)
      .catch(() => setSize(null));
  }, [folderId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const run = async () => {
    if (dest === null) return;
    setBusy(true);
    try {
      await invoke("folder_offload", { folderId, destLibraryId: dest });
      onClose();
    } catch (e) {
      toast(String(e), "drop");
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[70] bg-canvas/80 backdrop-blur-sm flex items-center justify-center p-6">
      <div role="dialog" aria-modal="true" aria-label={`「${name}」을 다른 디스크로`} className="w-[520px] max-w-full bg-chrome rounded-xl ring-1 ring-line shadow-2xl p-5">
        <div className="text-[16px] font-semibold text-fg mb-1">
          「{name}」을 다른 디스크로
        </div>
        <div className="text-[13px] text-fg-mute mb-4">
          {size
            ? `폴더 ${size.folders}개 · ${size.files.toLocaleString()}장 · ${fmtBytes(size.bytes)}. 라이브러리 안의 자리는 그대로 두고 통째로 옮깁니다. 목록에는 계속 보입니다.`
            : "…"}
        </div>

        <div className="text-[11.5px] uppercase tracking-wider text-fg-mute mb-1.5">
          어디로
        </div>
        {targets.length === 0 ? (
          <div className="text-[13px] text-fg-mute mb-4">
            옮겨 갈 다른 라이브러리가 없습니다. 아카이브 디스크의 폴더를 먼저
            라이브러리로 등록하세요(역할 «기타»).
          </div>
        ) : (
          <div className="flex flex-col gap-1.5 mb-4">
            {targets.map((l) => (
              <button
                key={l.id}
                disabled={!l.online}
                onClick={() => setDest(l.id)}
                className={`text-left px-3 py-2 rounded-md ring-1 disabled:opacity-40 ${
                  dest === l.id
                    ? "ring-accent bg-accent/15"
                    : "ring-line hover:bg-hover"
                }`}
              >
                <div className="text-[14px] text-fg">
                  <span className="text-fg-mute mr-1.5">
                    {areaLabel(l.area)}
                  </span>
                  {l.name}
                  {!l.online && (
                    <span className="text-fg-faint ml-2">연결 안 됨</span>
                  )}
                </div>
                <div className="text-[12px] text-fg-mute truncate">
                  {l.dir ?? l.volume_name}
                </div>
              </button>
            ))}
          </div>
        )}

        {synced && (
          <div className="mb-3 px-3 py-2 rounded-md text-[13px] text-drop ring-1 ring-drop/50 bg-drop/10">
            이 라이브러리는 내사진/공용 — Drive Client가 NAS와 양방향으로 맞추는
            폴더입니다. 여기서 옮기면 <b>NAS에서도 지워집니다.</b> 자리를 내려면
            Drive Client의 선택 동기화(폴더 해제)를 쓰세요.
          </div>
        )}
        <div className="text-[12.5px] text-fg-mute mb-4">
          다른 디스크면 파일마다 복사해 확인한 뒤 원본을 지웁니다. 중간에 멈추면
          복사한 것을 지우고 원본은 그대로 둡니다.
        </div>

        <div className="flex justify-end gap-2">
          <Btn onClick={onClose} hint="Esc">
            취소
          </Btn>
          <Btn tone="accent" disabled={dest === null || busy} onClick={run}>
            {busy ? "옮기는 중…" : "옮기기"}
          </Btn>
        </div>
      </div>
    </div>
  );
}
