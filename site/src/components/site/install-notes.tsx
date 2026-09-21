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

/** 安装说明里出现的文件名必须跟着版本走，所以整段都是按 version 现算的 */
function notesFor(platform: "windows" | "macos" | "linux", version: string): Note[] {
  const v = version || "0.1.x";
  if (platform === "windows") {
    return [
      {
        title: "安装向导（.exe，推荐）",
        body: "双击运行，安装器会自动处理提权并创建开始菜单 / 桌面快捷方式；之后的应用内自动更新也走这个包。",
        warn: "安装包未做代码签名，SmartScreen 可能提示「Windows 已保护你的电脑」：点「更多信息」→「仍要运行」即可。",
      },
      {
        title: "MSI 静默部署",
        body: "给企业环境用组策略或命令行批量安装时选 .msi：",
        code: `msiexec /i DSH.Launcher_${v}_x64_en-US.msi /qn`,
      },
      {
        title: "首次启动",
        body: "启动器会探测机器上的 Node / npm / dsh。一个都没有也能用：设置里可一键下载 Node LTS 到 ~/.dsh-launcher/runtime，用户级安装、不需要管理员权限。",
      },
    ];
  }
  if (platform === "macos") {
    return [
      {
        title: "拖进「应用程序」",
        body: "打开 .dmg，把 DSH Launcher 拖到「应用程序」。这个包是通用二进制，Apple Silicon 与 Intel 用的是同一个文件。",
      },
      {
        title: "首次打开被系统拦下",
        body: "应用没有做 Apple 公证，首次打开可能提示「无法验证开发者」。在「系统设置 → 隐私与安全性」里点「仍要打开」，或者执行：",
        code: `xattr -dr com.apple.quarantine "/Applications/DSH Launcher.app"`,
      },
      {
        title: "自动更新包不用手装",
        body: ".app.tar.gz 是 Tauri updater 的签名产物，客户端会在应用内自己下载替换；人手安装请用 .dmg。",
      },
    ];
  }
  return [
    {
      title: "AppImage（免安装）",
      body: "一个文件就是整份程序，不需要 root，也不会写系统目录：",
      code: `chmod +x DSH.Launcher_${v}_amd64.AppImage\n./DSH.Launcher_${v}_amd64.AppImage`,
      warn: "极简发行版可能缺 FUSE，装一下 libfuse2（Debian/Ubuntu）即可运行。",
    },
    {
      title: "Debian / Ubuntu",
      body: "用 apt 安装可以把依赖一起带上（dpkg -i 不会自动补依赖）：",
      code: `sudo apt install ./DSH.Launcher_${v}_amd64.deb`,
    },
    {
      title: "Fedora / openSUSE",
      body: "rpm 系发行版用 -U 升级安装：",
      code: `sudo rpm -U DSH.Launcher-${v}-1.x86_64.rpm`,
    },
    {
      title: "更新方式的差别",
      body: "AppImage 可以在原位静默替换自己；.deb / .rpm 属于系统级安装，更新时会弹出授权窗口（pkexec / sudo），必须有人在电脑前。",
    },
  ];
}

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
          lead="安装包不带运行时依赖，也不需要管理员权限（系统级安装包除外）。下面是每个平台最常见的两个坑：代码签名提示与 FUSE 依赖。"
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <Tabs defaultValue="windows">
          <TabsList variant="line" className="gap-6 border-b border-hairline pb-0">
            <TabsTrigger value="windows" className="gap-2 text-[13.5px]">
              <PlatformMark platform="windows" className="size-3.5" />
              Windows
            </TabsTrigger>
            <TabsTrigger value="macos" className="gap-2 text-[13.5px]">
              <PlatformMark platform="macos" className="size-3.5" />
              macOS
            </TabsTrigger>
            <TabsTrigger value="linux" className="gap-2 text-[13.5px]">
              <PlatformMark platform="linux" className="size-3.5" />
              Linux
            </TabsTrigger>
          </TabsList>

          {(["windows", "macos", "linux"] as const).map((platform) => (
            <TabsContent key={platform} value={platform} className="pt-8">
              <ol className="space-y-4">
                {notesFor(platform, version).map((note, i) => (
                  <li key={note.title} className="panel p-5">
                    <div className="flex items-baseline gap-3">
                      <span className="eyebrow shrink-0 text-primary">步骤 {i + 1}</span>
                      <h3 className="text-[15px] font-medium">{note.title}</h3>
                    </div>
                    {note.body ? (
                      <p className="mt-2.5 max-w-[74ch] text-[13.5px] text-muted-foreground">{note.body}</p>
                    ) : null}
                    {note.code ? <CodeBlock code={note.code} /> : null}
                    {note.warn ? (
                      <p className="mt-3 flex items-start gap-2 text-[12.5px] text-warn">
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

      <Reveal delay={120} className="mt-6">
        <div className="panel flex flex-wrap items-start gap-3 p-5">
          <TerminalIcon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0 flex-1">
            <p className="text-[13.5px]">
              装完不用配环境变量。启动器把 dsh 版本装到{" "}
              <code className="num rounded bg-muted px-1.5 py-0.5 text-[12.5px]">~/.dsh-launcher/versions/&lt;版本&gt;</code>
              ，把内置 Node 放在同级的{" "}
              <code className="num rounded bg-muted px-1.5 py-0.5 text-[12.5px]">runtime/</code>
              ，互不干扰、也不污染全局 npm。
            </p>
            <p className="mt-2 max-w-[80ch] text-[12.5px] text-muted-foreground">
              dsh 自己的数据目录仍是{" "}
              <code className="num text-[12px]">$DSH_HOME</code>（默认{" "}
              <code className="num text-[12px]">~/.dsh</code>）：profile、插件与凭据都在那儿 ——
              全新机器上第一次运行 dsh 才会生成，启动器的首次使用引导会替你跑这一步。
            </p>
          </div>
        </div>
      </Reveal>
    </section>
  );
}

function CodeBlock({ code }: { code: string }) {
  return (
    <div className="group/code mt-3 flex items-start gap-3 rounded-lg border border-hairline bg-ink/60 px-3.5 py-3">
      <pre className="num min-w-0 flex-1 overflow-x-auto text-[12.5px] leading-6 whitespace-pre text-foreground/90">
        {code}
      </pre>
      <CopyButton text={code} iconOnly label="复制命令" variant="ghost" className="text-muted-foreground" />
    </div>
  );
}
