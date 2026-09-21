/*
 * 发布数据层：页面上的版本号、体积、直链全部是**运行时拉来的真实数据**。
 *
 * 数据来源与它们各自解决的事：
 * - Cloudflare R2（自建源，客户端默认更新源）：桶里是**与版本无关的固定键**
 *   （`latest/windows-x64-setup.exe`）—— 所以即使一个版本号都没读到，直链也照样能下。
 *   清单 `latest.json` 是权威的 version / pub_date / notes（Tauri 更新器格式）。
 *   注意：这个桶没有配 CORS，浏览器**读不到**清单内容（启动器走 Rust HTTP 客户端，不受限制），
 *   所以「可达」与「可读」在状态里是两件事。
 * - GitHub Release：版本号、发布日期、体积与下载次数（assets[].size），以及更新说明原文
 *   （release 正文由 CHANGELOG 派生，与自建源清单里的 notes 同源）。API 带
 *   `access-control-allow-origin: *`，浏览器能直接读。
 *
 * 任一源挂了页面都还能用；两条都挂时自建源的固定键直链依然可用 ——
 * 页面会说清「现在能确定什么」，而不是装作一切正常。
 */

export const REPO = "Yinxe/dsh-luncher";
export const REPO_URL = `https://github.com/${REPO}`;

/** 与 src-tauri/src/update_check.rs 的 R2_BASE 保持一致（改一处就得改两处） */
export const R2_BASE = "https://pub-65e25af191f546ddb6c2d4fa976345c7.r2.dev";
export const R2_MANIFEST_URL = `${R2_BASE}/latest.json`;
export const GH_API_URL = `https://api.github.com/repos/${REPO}/releases/latest`;
export const GH_RELEASES_URL = `${REPO_URL}/releases`;
export const CHANGELOG_URL = `${REPO_URL}/blob/main/CHANGELOG.md`;

export type SourceId = "r2" | "github";
export type PlatformId = "windows" | "macos" | "linux" | "unknown";

export interface AssetDef {
  id: string;
  platform: PlatformId;
  /** 人类读的扩展名，用于徽标 */
  ext: string;
  /** 这个包叫什么（表里的主标题） */
  label: string;
  arch: string;
  /** GitHub 资产原始文件名（带版本号，与 tauri-bundler 的输出一致） */
  ghName: (version: string) => string;
  /** R2 上的固定对象键；null = 自建源不托管这个包 */
  r2Key: string | null;
  recommended?: boolean;
  /** 一句话说清「该选哪个」 */
  blurb: string;
  /** 自动更新产物，不是给人手装的 */
  updaterOnly?: boolean;
}

export const ASSETS: AssetDef[] = [
  {
    id: "win-exe",
    platform: "windows",
    ext: "exe",
    label: "Windows 安装向导（NSIS）",
    arch: "x64",
    ghName: (v) => `DSH.Launcher_${v}_x64-setup.exe`,
    r2Key: "windows-x64-setup.exe",
    recommended: true,
    blurb: "双击安装，自动建快捷方式；应用内更新也走这个包。",
  },
  {
    id: "win-msi",
    platform: "windows",
    ext: "msi",
    label: "Windows MSI 安装包",
    arch: "x64",
    ghName: (v) => `DSH.Launcher_${v}_x64_en-US.msi`,
    r2Key: "windows-x64.msi",
    blurb: "企业环境用 msiexec、组策略批量部署。",
  },
  {
    id: "linux-appimage",
    platform: "linux",
    ext: "AppImage",
    label: "Linux AppImage（免安装）",
    arch: "amd64",
    ghName: (v) => `DSH.Launcher_${v}_amd64.AppImage`,
    r2Key: "linux-x86_64.AppImage",
    recommended: true,
    blurb: "一个文件就是整份程序：chmod +x 直接跑，自更新原位替换，不需要 root。",
  },
  {
    id: "linux-deb",
    platform: "linux",
    ext: "deb",
    label: "Debian / Ubuntu 软件包",
    arch: "amd64",
    ghName: (v) => `DSH.Launcher_${v}_amd64.deb`,
    r2Key: "linux-x86_64.deb",
    blurb: "包管理器负责依赖与卸载；系统级安装，更新时需要授权。",
  },
  {
    id: "linux-rpm",
    platform: "linux",
    ext: "rpm",
    label: "Fedora / openSUSE 软件包",
    arch: "x86_64",
    ghName: (v) => `DSH.Launcher-${v}-1.x86_64.rpm`,
    r2Key: "linux-x86_64.rpm",
    blurb: "rpm -U 安装，行为同 .deb。",
  },
  {
    id: "mac-dmg",
    platform: "macos",
    ext: "dmg",
    label: "macOS 磁盘映像（通用二进制）",
    arch: "universal — Apple Silicon + Intel",
    ghName: (v) => `DSH.Launcher_${v}_universal.dmg`,
    r2Key: null,
    recommended: true,
    blurb: "Apple Silicon 与 Intel 通用，拖进「应用程序」即可。",
  },
  {
    id: "mac-app-tar",
    platform: "macos",
    ext: "app.tar.gz",
    label: "macOS 自动更新包",
    arch: "universal",
    ghName: (v) => `DSH.Launcher_${v}_universal.app.tar.gz`,
    r2Key: "darwin-universal.app.tar.gz",
    updaterOnly: true,
    blurb: "Tauri 更新器专用产物，由客户端在应用内替换 .app，不用手装。",
  },
];

/**
 * 主推包：当前平台标了「推荐」的那个 → 该平台第一个 → 全表推荐的第一个。
 * 首屏大按钮与「复制直链」都用它，所以这个顺序必须是确定性的。
 */
export function pickPrimary<T extends AssetDef>(rows: T[], platform: PlatformId): T | undefined {
  const usable = rows.filter((r) => !r.updaterOnly);
  const samePlatform = usable.filter((r) => r.platform === platform);
  return (
    samePlatform.find((r) => r.recommended) ??
    samePlatform[0] ??
    usable.find((r) => r.recommended) ??
    usable[0]
  );
}

export interface GhAsset {
  name: string;
  size: number;
  downloads: number;
}

/**
 * 一条源的可用程度。
 *
 * 为什么不是简单的「通 / 不通」：自建源是 Cloudflare R2 的公开桶，默认**没有配 CORS**，
 * 于是浏览器能发出请求、能下载文件，却读不到 latest.json 的内容（那份清单是给启动器用的，
 * 它走 Rust 的 HTTP 客户端，不受 CORS 限制）。所以「可达」与「可读」必须分开：
 * 前者代表用户点下载没问题，后者才代表页面能读到清单原文。
 */
export type SourceStatus = "probing" | "readable" | "reachable" | "down";

export interface ReleaseState {
  /** null = 两条源都没读到版本号 */
  version: string | null;
  publishedAt: string | null;
  /** 更新说明原文（优先取自建源清单，其次 GitHub Release 正文，两者同源） */
  notes: string | null;
  notesFrom: "r2" | "github" | null;
  /** 两条源都给出过版本号且不一致（自建源可能还没同步） */
  versionMismatch: boolean;
  r2: SourceStatus;
  github: SourceStatus;
  /** 只有桶开了 CORS 才拿得到，用来核对两条源是否同步 */
  r2Version: string | null;
  assets: GhAsset[];
  /** 拉取结束（无论成败），用来切换骨架屏与状态灯 */
  settled: boolean;
}

export const EMPTY_RELEASE: ReleaseState = {
  version: null,
  publishedAt: null,
  notes: null,
  notesFrom: null,
  versionMismatch: false,
  r2: "probing",
  github: "probing",
  r2Version: null,
  assets: [],
  settled: false,
};

/** 状态灯配色：可读/可达都算正常，只有真的不可达才报警 */
export function statusTone(status: SourceStatus): "signal" | "warn" | "muted" {
  if (status === "probing") return "muted";
  return status === "down" ? "warn" : "signal";
}

/** 状态文案：把「可达但不给读」说清楚，而不是笼统写「异常」 */
export function statusLabel(status: SourceStatus, version?: string | null): string {
  switch (status) {
    case "probing":
      return "探测中";
    case "readable":
      return version ? `v${version}` : "清单可读";
    case "reachable":
      return "可达 · 未开 CORS";
    case "down":
      return "不可达";
  }
}

/**
 * 自建源桶是否已经配了 CORS（**默认没有**）。
 *
 * 为什么要有这个常量：桶没开 CORS 时，浏览器对 latest.json 的 `fetch`（cors 模式）
 * 必然失败并在控制台留下一条「blocked by CORS policy」——而这条信息对用户毫无意义：
 * 文件能下、启动器能更新，只有这个网页读不到清单内容。所以默认不去读它，
 * 只用 no-cors 探一下可达性；**给桶配上 CORS 后把这个常量改成 true**，
 * 页面就会改用它作为权威的版本号与更新说明来源（并开始核对两条源是否同步）。
 *
 * 怎么配（Cloudflare 控制台 → R2 → 该桶 → Settings → CORS Policy，或 API）：
 *   [{ "AllowedOrigins": ["https://yinxe.github.io"],
 *      "AllowedMethods": ["GET", "HEAD"], "AllowedHeaders": ["*"] }]
 */
const R2_CORS_ENABLED = false;

/** 8 秒拿不到就放弃 —— 下载页不该被一个卡住的接口拖住首屏 */
async function getJson<T>(url: string, timeoutMs = 8000): Promise<T> {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const res = await fetch(url, { signal: ac.signal, cache: "no-store" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as T;
  } finally {
    clearTimeout(timer);
  }
}

/**
 * 只探可达性、不读内容。
 *
 * `mode: "no-cors"` 在跨域且对方没给 CORS 头时返回 opaque 响应：内容拿不到，但
 * **请求确实完成了**（DNS / TLS / HTTP 都正常）—— 这正是「用户能不能下到这个文件」
 * 的答案。桶以后开了 CORS 也不会变差：那时 readable 分支会接管。
 */
async function probe(url: string, timeoutMs = 8000): Promise<boolean> {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    await fetch(url, { mode: "no-cors", cache: "no-store", signal: ac.signal });
    return true;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

/** GitHub API 的资产原始字段（只用到这三个） */
interface GhApiAsset {
  name?: string;
  size?: number;
  download_count?: number;
}

/**
 * 取发布信息：三个请求并行，各自 try/catch ——
 * 一条源挂了不影响另一条，页面永远渲染得出「能用的那部分」。
 */
export async function fetchRelease(): Promise<ReleaseState> {
  const [r2Data, r2Reachable, gh] = await Promise.all([
    R2_CORS_ENABLED
      ? getJson<{ version?: string; notes?: string; pub_date?: string }>(R2_MANIFEST_URL).catch(() => null)
      : Promise.resolve(null),
    probe(R2_MANIFEST_URL),
    getJson<{ tag_name?: string; published_at?: string; body?: string; assets?: GhApiAsset[] }>(
      GH_API_URL,
    ).catch(() => null),
  ]);

  const r2Version = r2Data?.version?.trim() || null;
  const ghVersion = gh?.tag_name?.replace(/^v/, "").trim() || null;
  const r2Notes = r2Data?.notes?.trim() || null;
  const ghNotes = gh?.body?.trim() || null;

  return {
    version: r2Version ?? ghVersion,
    publishedAt: r2Data?.pub_date ?? gh?.published_at ?? null,
    notes: r2Notes ?? ghNotes,
    notesFrom: r2Notes ? "r2" : ghNotes ? "github" : null,
    versionMismatch: Boolean(r2Version && ghVersion && r2Version !== ghVersion),
    r2: r2Version ? "readable" : r2Reachable ? "reachable" : "down",
    github: ghVersion ? "readable" : gh ? "reachable" : "down",
    r2Version,
    assets: (gh?.assets ?? [])
      .filter((a): a is { name: string; size?: number; download_count?: number } =>
        Boolean(a && typeof a.name === "string"),
      )
      .map((a) => ({ name: a.name, size: a.size ?? 0, downloads: a.download_count ?? 0 })),
    settled: true,
  };
}

/**
 * 自建源直链：路径**与版本无关**，永远指向「当前最新」。
 * `?v=` 是发布流程写进清单的缓存指纹（换版本换 URL，好让 CDN 放心长缓存）；
 * 版本号没读到时不带指纹也能下，只是可能命中边缘缓存。
 */
export function r2Url(key: string, version: string | null): string {
  return `${R2_BASE}/latest/${encodeURIComponent(key)}${version ? `?v=${encodeURIComponent(version)}` : ""}`;
}

/** GitHub 直链：与版本绑定，永久有效（适合固定版本归档 / 写进脚本） */
export function ghUrl(name: string, version: string | null): string | null {
  if (!version) return null;
  return `${REPO_URL}/releases/download/v${version}/${encodeURIComponent(name)}`;
}

export interface ResolvedLink {
  /** 当前源下这个包的下载地址；null = 该源不提供 */
  url: string | null;
  /** 地址为什么不可用的原因（给界面显示，不要只说「不可用」） */
  unavailable?: string;
}

/** 按「当前源」解析一个包的实际下载地址 */
export function resolveDownload(asset: AssetDef, source: SourceId, version: string | null): ResolvedLink {
  if (source === "r2") {
    if (!asset.r2Key) {
      return { url: null, unavailable: "自建源未托管这个包，请切到 GitHub 源" };
    }
    return { url: r2Url(asset.r2Key, version) };
  }
  const url = ghUrl(asset.ghName(version ?? ""), version);
  return url
    ? { url }
    : { url: null, unavailable: "还没读到版本号，GitHub 直链需要版本号；可改用自建源固定键" };
}

/** 体积：从 GitHub 资产映射表里取（拿不到就返回 null，界面显示 —） */
export function sizeOf(state: ReleaseState, asset: AssetDef, version: string | null): number | null {
  if (!version) return null;
  const hit = state.assets.find((a) => a.name === asset.ghName(version));
  return hit && hit.size > 0 ? hit.size : null;
}

export function detectPlatform(): PlatformId {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent;
  if (/Mac OS X|Macintosh|iPhone|iPad/.test(ua)) return "macos";
  if (/Windows|Win32|Win64/.test(ua)) return "windows";
  if (/Linux|X11|CrOS/.test(ua)) return "linux";
  return "unknown";
}

export const PLATFORM_LABEL: Record<PlatformId, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
  unknown: "你的平台",
};
