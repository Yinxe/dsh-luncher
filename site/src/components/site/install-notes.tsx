import { TerminalIcon, TriangleAlertIcon } from "lucide-react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CopyButton } from "@/components/site/copy-button";
import { PlatformMark } from "@/components/site/icons";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import type { ReleaseState } from "@/lib/release";

interface Note {
  title: string;
  body?: string;
  code?: string;
  warn?: string;
}

/** 安装说明里的文件名必须跟着版本走，所以整段都是按 version 现算的 */
function notesFor(platform: "windows" | "macos" | "linux", version: string): Note[] {
  const v = version || "0.1.x";
  if (platform === "windows") {
    return [
      {
        title: "安装向导（.exe，推荐）",
        body: "双击运行，安装器会处理提权并创建快捷方式；之后的应用内自动更新也走这个包。",
        warn: "安装包未做代码签名，SmartScreen 可能提示「Windows 已保护你的电脑」：点「更多信息 → 仍要运行」即可。",
      },
      {
        title: "MSI 静默部署",
        body: "组策略或命令行批量安装时选 .msi：",
        code: `msiexec /i DSH.Starter_${v}_x64_en-US.msi /qn`,
      },
      {
        title: "首次启动",
        body: "启动器会探测机器上的 Node / npm / dsh；一个都没有也能用 —— 设置里可一键下载 Node LTS 到自己的数据目录，免管理员权限。",
      },
    ];
  }
  if (platform === "macos") {
    return [
      {
        title: "拖进「应用程序」",
        body: "打开 .dmg，把 DSH Starter 拖进「应用程序」。通用二进制，Apple Silicon 与 Intel 用同一个文件。",
      },
      {
        title: "首次打开被系统拦下",
        body: "应用没有做 Apple 公证，可能提示「无法验证开发者」。在「系统设置 → 隐私与安全性」点「仍要打开」，或者执行：",
        code: `xattr -dr com.apple.quarantine "/Applications/DSH Starter.app"`,
      },
      {
        title: "自动更新包不用手装",
        body: ".app.tar.gz 是客户端在应用内替换 .app 用的产物，人手安装请用 .dmg。",
      },
    ];
  }
  return [
    {
      title: "AppImage（免安装）",
      body: "一个文件就是整份程序，不需要 root，也不写系统目录：",
      code: `chmod +x DSH.Starter_${v}_amd64.AppImage\n./DSH.Starter_${v}_amd64.AppImage`,
      warn: "极简发行版可能缺 FUSE，装一下 libfuse2（Debian / Ubuntu）即可运行。",
    },
    {
      title: "Debian / Ubuntu",
      body: "用 apt 安装可以把依赖一起带上（dpkg -i 不会自动补依赖）：",
      code: `sudo apt install ./DSH.Starter_${v}_amd64.deb`,
    },
    {
      title: "Fedora / openSUSE",
      body: "rpm 系发行版用 -U 升级安装：",
      code: `sudo rpm -U DSH.Starter-${v}-1.x86_64.rpm`,
    },
    {
      title: "更新方式的差别",
      body: "AppImage 可以在原位静默替换自己；.deb / .rpm 属于系统级安装，更新时会弹授权窗口，得有人在电脑前。",
    },
  ];
}

const TABS = [
  { id: "windows", label: "Windows" },
  { id: "macos", label: "macOS" },
  { id: "linux", label: "Linux" },
] as const;

export function InstallNotes({ state }: { state: ReleaseState }) {
  const version = state.version ?? "0.1.x";

  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="02"
          kicker="安装"
          id="install"
          title="三个平台，各三步"
          lead="安装包自带运行时探测，不需要管理员权限（系统级安装包除外）。每个平台最容易被系统拦下的地方，下面都给了处理办法。"
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <Tabs defaultValue="windows">
          <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b border-hairline">
            <TabsList variant="line" className="h-9 gap-6 rounded-none p-0">
              {TABS.map((tab) => (
                <TabsTrigger key={tab.id} value={tab.id} className="gap-2 rounded-none px-0 text-[13.5px]">
                  <PlatformMark platform={tab.id} className="size-3.5" />
                  {tab.label}
                </TabsTrigger>
              ))}
            </TabsList>
            <span className="eyebrow hidden sm:inline">安装包 5 – 86 MB</span>
          </div>

          {TABS.map((tab) => (
            <TabsContent key={tab.id} value={tab.id} className="pt-2">
              <ol className="grid gap-x-12 lg:grid-cols-2">
                {notesFor(tab.id, version).map((note, i) => (
                  <li key={note.title} className="min-w-0 border-t border-hairline py-6">
                    <div className="flex items-baseline gap-3">
                      <span className="num text-[11.5px] text-primary">
                        {String(i + 1).padStart(2, "0")}
                      </span>
                      <h3 className="text-[15px] font-medium">{note.title}</h3>
                    </div>
                    {note.body ? (
                      <p className="mt-2 ml-7 max-w-[62ch] text-[13.5px] break-words text-muted-foreground">{note.body}</p>
                    ) : null}
                    {note.code ? <CodeBlock code={note.code} /> : null}
                    {note.warn ? (
                      <p className="mt-3 ml-7 flex items-start gap-2 text-[12.5px] break-words text-warn">
                        <TriangleAlertIcon className="mt-[3px] size-3.5 shrink-0" />
                        <span>{note.warn}</span>
                      </p>
                    ) : null}
                  </li>
                ))}
              </ol>
            </TabsContent>
          ))}
        </Tabs>
      </Reveal>

      {/* 数据目录：两个路径分清楚，这是新用户最容易混的地方 */}
      <Reveal delay={120} className="mt-6 grid gap-x-12 gap-y-6 border-t border-hairline pt-8 lg:grid-cols-2">
        <div className="flex items-start gap-3">
          <TerminalIcon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div>
            <h3 className="text-[14px] font-medium">启动器的数据</h3>
            <p className="mt-1.5 text-[13px] text-muted-foreground">
              <code className="num rounded bg-code px-1.5 py-0.5 text-[12.5px]">~/.dsh-starter/</code> —— 版本装在{" "}
              <span className="num text-[12.5px]">versions/&lt;版本&gt;</span>、内置 Node 在{" "}
              <span className="num text-[12.5px]">runtime/</span>、日志在{" "}
              <span className="num text-[12.5px]">logs/</span>。各版本互不干扰，也不污染全局 npm。
            </p>
          </div>
        </div>
        <div className="flex items-start gap-3">
          <TerminalIcon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div>
            <h3 className="text-[14px] font-medium">dsh 自己的数据</h3>
            <p className="mt-1.5 text-[13px] text-muted-foreground">
              <code className="num rounded bg-code px-1.5 py-0.5 text-[12.5px]">$DSH_HOME</code>（默认{" "}
              <span className="num text-[12.5px]">~/.dsh</span>）—— profile、插件与凭据都在那儿；全新机器上第一次
              运行 dsh 才会生成，启动器的首次使用引导会替你跑这一步。
            </p>
          </div>
        </div>
      </Reveal>
    </section>
  );
}

function CodeBlock({ code }: { code: string }) {
  return (
    <div className="mt-3 ml-7 flex min-w-0 items-start gap-3 rounded-lg border border-hairline bg-code px-3.5 py-3">
      <pre className="num min-w-0 flex-1 overflow-x-auto text-[12.5px] leading-6 whitespace-pre text-foreground/90">
        {code}
      </pre>
      <CopyButton text={code} iconOnly label="复制命令" variant="ghost" className="text-muted-foreground" />
    </div>
  );
}
