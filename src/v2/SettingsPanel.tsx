import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { Btn } from "./ui";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";

function Head({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pt-3 pb-1 text-[10.5px] uppercase tracking-wider text-fg-mute">
      {children}
    </div>
  );
}

/**
 * 설정 — 지금은 썸네일 캐시와 앱 정보뿐이다.
 *
 * 다른 갈래와 달리 사진을 거르지 않는다. 레일 아래쪽에 휴지통과 같이 두는
 * 이유다.
 */
export default function SettingsPanel({
  thumbBytes,
  onRefresh,
}: {
  /** 썸네일이 쓰는 용량. 아직 안 셌으면 null */
  thumbBytes: number | null;
  onRefresh: () => void;
}) {
  const ask = useConfirm();
  const [ver, setVer] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getVersion()
      .then(setVer)
      .catch(() => setVer(""));
  }, []);

  return (
    <>
      <Head>썸네일 캐시</Head>
      <div className="px-3 text-[12px] text-fg-dim tabular-nums">
        {thumbBytes === null ? "—" : fmtBytes(thumbBytes)}
      </div>
      <div className="px-2 pt-2 flex gap-1">
        <Btn onClick={onRefresh}>다시 세기</Btn>
        <Btn
          tone="drop"
          disabled={busy}
          onClick={async () => {
            const ok = await ask({
              title: "썸네일을 모두 지웁니다",
              lines: [
                "· 사진은 그대로입니다",
                "· 다음에 볼 때 다시 만들어집니다 — 12만 장이면 한참 걸립니다",
              ],
              confirmLabel: "비우기",
              danger: true,
            });
            if (!ok) return;
            setBusy(true);
            try {
              await invoke("cache_clear");
              onRefresh();
            } finally {
              setBusy(false);
            }
          }}
        >
          비우기
        </Btn>
      </div>

      <Head>에이컷</Head>
      <div className="px-3 text-[12px] text-fg-dim leading-relaxed">
        버전 {ver || "—"}
        <br />
        <span className="text-fg-mute">사진은 원래 자리에 그대로 둡니다.</span>
      </div>
    </>
  );
}
