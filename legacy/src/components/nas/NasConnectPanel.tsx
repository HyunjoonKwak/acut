import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Server, LogOut, Loader2, KeyRound } from "lucide-react";
import type { NasConfig, NasStatus } from "@/types";

interface Props {
  config: NasConfig | null;
  status: NasStatus;
  uploading: boolean;
  onStatusChange: (status: NasStatus) => void;
}

export function NasConnectPanel({
  config,
  status,
  uploading,
  onStatusChange,
}: Props) {
  const [url, setUrl] = useState("");
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [otpCode, setOtpCode] = useState("");
  const [verifyTls, setVerifyTls] = useState(true);
  const [needOtp, setNeedOtp] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (config) {
      setUrl((prev) => prev || config.url);
      setAccount((prev) => prev || config.account);
      setVerifyTls(config.verify_tls);
    }
  }, [config]);

  const handleConnect = async () => {
    if (!url.trim() || !account.trim() || !password) {
      setError("주소, 계정, 비밀번호를 입력해주세요");
      return;
    }
    setConnecting(true);
    setError(null);
    try {
      const next = await invoke<NasStatus>("nas_connect", {
        url: url.trim(),
        account: account.trim(),
        password,
        otpCode: otpCode.trim() || null,
        verifyTls,
      });
      setPassword("");
      setOtpCode("");
      setNeedOtp(false);
      onStatusChange(next);
    } catch (e) {
      const message = String(e);
      // DSM error 403: OTP required for this account
      if (message.includes("[403]")) {
        setNeedOtp(true);
      }
      setError(message);
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = async () => {
    try {
      await invoke("nas_disconnect");
    } catch {
      // Session already gone on the NAS side; treat as disconnected
    }
    onStatusChange({ connected: false, account: null, url: null });
  };

  if (status.connected) {
    return (
      <div className="p-4 border-b border-border">
        <div className="flex items-center gap-2 mb-2">
          <Server size={14} className="text-success" />
          <span className="text-[12px] font-semibold text-text-primary">
            NAS 연결됨
          </span>
        </div>
        <p className="text-[11px] text-text-secondary truncate">
          {status.account} @ {status.url}
        </p>
        <button
          onClick={handleDisconnect}
          disabled={uploading}
          title={uploading ? "업로드 중에는 연결을 해제할 수 없습니다" : undefined}
          className="mt-3 flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[11px] text-text-secondary border border-border hover:bg-bg-primary hover:text-text-primary disabled:opacity-40 disabled:pointer-events-none transition-colors"
        >
          <LogOut size={12} />
          연결 해제
        </button>
      </div>
    );
  }

  return (
    <div className="p-4 border-b border-border">
      <div className="flex items-center gap-2 mb-3">
        <Server size={14} className="text-accent" />
        <span className="text-[12px] font-semibold text-text-primary">
          Synology NAS 연결
        </span>
      </div>

      <div className="space-y-2">
        <input
          type="text"
          placeholder="http://192.168.0.10:5000"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          className="w-full h-7 px-2.5 rounded-md text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
        />
        <input
          type="text"
          placeholder="DSM 계정"
          value={account}
          onChange={(e) => setAccount(e.target.value)}
          autoCapitalize="off"
          autoCorrect="off"
          className="w-full h-7 px-2.5 rounded-md text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
        />
        <input
          type="password"
          placeholder="비밀번호 (저장되지 않음)"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleConnect()}
          className="w-full h-7 px-2.5 rounded-md text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
        />
        {needOtp && (
          <div className="relative">
            <KeyRound
              size={12}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-secondary"
            />
            <input
              type="text"
              placeholder="2단계 인증(OTP) 코드"
              value={otpCode}
              onChange={(e) => setOtpCode(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleConnect()}
              className="w-full h-7 pl-8 pr-2.5 rounded-md text-[11px] bg-bg-primary border border-border text-text-primary placeholder:text-text-secondary/50 focus:outline-none focus:ring-1 focus:ring-accent/50"
            />
          </div>
        )}
        <label className="flex items-center gap-1.5 text-[10px] text-text-secondary cursor-pointer">
          <input
            type="checkbox"
            checked={verifyTls}
            onChange={(e) => setVerifyTls(e.target.checked)}
            className="accent-[var(--color-accent)]"
          />
          TLS 인증서 검증 (자체 서명 인증서면 끄세요)
        </label>
        {url.trim().startsWith("http://") && (
          <p className="text-[9px] text-warning leading-relaxed">
            HTTP 연결은 암호화되지 않습니다. 같은 네트워크에서 비밀번호가
            노출될 수 있으니 가능하면 HTTPS(5001 포트)를 사용하세요.
          </p>
        )}
      </div>

      {error && (
        <p className="mt-2 text-[10px] text-danger leading-relaxed break-all">
          {error}
        </p>
      )}

      <button
        onClick={handleConnect}
        disabled={connecting}
        className="mt-3 w-full flex items-center justify-center gap-1.5 h-7 rounded-md text-[11px] font-medium bg-accent text-white hover:bg-accent-hover disabled:opacity-50 transition-colors"
      >
        {connecting ? (
          <>
            <Loader2 size={12} className="animate-spin" />
            연결 중...
          </>
        ) : (
          "연결"
        )}
      </button>
      <p className="mt-2 text-[9px] text-text-secondary/70 leading-relaxed">
        본인 DSM 계정으로 로그인합니다. 비밀번호는 저장되지 않고 세션에만
        사용됩니다.
      </p>
    </div>
  );
}
