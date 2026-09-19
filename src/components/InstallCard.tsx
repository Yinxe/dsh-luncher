import { useEffect, useRef } from "react";

interface Props {
  version: string;
  logs: string[];
  onCancel: () => void;
}

export default function InstallCard({ version, logs, onCancel }: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [logs]);

  return (
    <div className="install-card">
      <div className="row">
        <span className="spinner" />
        <strong>正在安装 dsh {version}</strong>
        <span style={{ color: "var(--text-dim)", fontSize: 12 }}>
          依赖较多，请耐心等待…
        </span>
        <span style={{ flex: 1 }} />
        <button className="sm danger" onClick={onCancel}>
          取消
        </button>
      </div>
      <div className="log" ref={logRef}>
        {logs.length === 0 ? "正在启动 npm…" : logs.join("\n")}
      </div>
    </div>
  );
}
