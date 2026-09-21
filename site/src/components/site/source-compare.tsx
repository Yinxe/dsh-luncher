import { ExternalLinkIcon } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { CopyButton } from "@/components/site/copy-button";
import { Led } from "@/components/site/led";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import type { ReleaseState } from "@/lib/release";
import { R2_MANIFEST_URL, REPO_URL } from "@/lib/release";

/**
 * 两条源的分工 —— 「可选 GitHub 或 R2」这件事本身需要解释清楚，
 * 因为它们的差别不只是「快慢」，而是**地址的生命周期**不同：
 * R2 的地址与版本无关（一条链接永远指向最新），GitHub 的地址与版本绑定（固定版本归档）。
 */
export function SourceCompare({ state }: { state: ReleaseState }) {
  const v = state.version ?? "0.1.7";
  const r2Sample = `…/latest/windows-x64-setup.exe?v=${v}`;
  const ghSample = `…/releases/download/v${v}/DSH.Launcher_${v}_x64-setup.exe`;

  const rows: {
    dim: string;
    r2: React.ReactNode;
    gh: React.ReactNode;
  }[] = [
    {
      dim: "地址形态",
      r2: <code className="num text-[11.5px] break-all">{r2Sample}</code>,
      gh: <code className="num text-[11.5px] break-all">{ghSample}</code>,
    },
    {
      dim: "长期有效性",
      r2: "一条链接长期有效，永远下到最新版（发布时同名覆盖）",
      gh: "每条链接固定一个版本，永久有效（归档首选）",
    },
    {
      dim: "国内直连",
      r2: (
        <span className="flex items-start gap-2">
          <Led tone="signal" className="mt-[7px]" />
          <span>项目实测（不走代理，拉 81MB AppImage）：可用，约 3.3MB/s</span>
        </span>
      ),
      gh: (
        <span className="flex items-start gap-2">
          <Led tone="warn" className="mt-[7px]" />
          <span>release 下载常连不上（实测返回 000），一般需要代理或加速前缀</span>
        </span>
      ),
    },
    {
      dim: "下载到的文件名",
      r2: "对象键是固定键，靠响应头还原原始资产名（如 DSH.Launcher_x.x.x_x64-setup.exe）",
      gh: "就是原始资产名，另附 .sig 签名文件",
    },
    {
      dim: "托管范围",
      r2: "6 个平台键：Windows .exe/.msi、Linux AppImage/.deb/.rpm、macOS 自动更新包（暂不含 .dmg）",
      gh: "全部产物：上面所有包 + macOS .dmg + 每个包的 .sig + latest.json",
    },
    {
      dim: "客户端里的角色",
      r2: (
        <span className="flex items-center gap-2">
          <Badge className="h-5 bg-primary/15 text-[11px] text-primary" variant="secondary">
            默认
          </Badge>
          更新时优先取这条源的清单
        </span>
      ),
      gh: "兜底：自建源取不到清单时自动回退到它",
    },
    {
      dim: "适合谁",
      r2: "写进脚本、贴给别人、页面上的「一键下载」—— 你不需要关心版本号变了没",
      gh: "固定版本复现问题、核对签名、翻历史版本",
    },
  ];

  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="03"
          kicker="更新源"
          id="sources"
          title="两条下载通道，各有各的用处"
          lead={
            <>
              启动器内置了两条更新通道，设置里二选一；本页顶部的切换器做的是同一件事。
              两条源上的安装包字节完全相同，客户端一律先用内置公钥验签 ——{" "}
              <strong className="text-foreground">所以走第三方镜像或代理也无法投毒</strong>：
              改一个字节就验签失败。
            </>
          }
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        {/* 窄屏：两列长文本塞进表格只能横向滚动，读起来很糟，改成逐维度堆叠 */}
        <div className="panel divide-y divide-hairline overflow-hidden md:hidden">
          {rows.map((row) => (
            <div key={row.dim} className="p-4">
              <div className="eyebrow text-foreground/70">{row.dim}</div>
              <dl className="mt-3 space-y-3">
                <div>
                  <dt className="flex items-center gap-2 text-[12px] text-muted-foreground">
                    <Led tone="signal" />
                    自建源 · Cloudflare R2
                  </dt>
                  <dd className="mt-1 text-[13px] text-muted-foreground">{row.r2}</dd>
                </div>
                <div>
                  <dt className="flex items-center gap-2 text-[12px] text-muted-foreground">
                    <Led tone="warn" />
                    GitHub Release
                  </dt>
                  <dd className="mt-1 text-[13px] text-muted-foreground">{row.gh}</dd>
                </div>
              </dl>
            </div>
          ))}
        </div>

        {/* 宽屏：真正的三列对照表 */}
        <div className="panel hidden overflow-hidden md:block">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="w-[128px] text-[12px]">维度</TableHead>
                <TableHead className="text-[12px]">
                  <span className="flex items-center gap-2">
                    <Led tone="signal" />
                    自建源 · Cloudflare R2
                  </span>
                </TableHead>
                <TableHead className="text-[12px]">
                  <span className="flex items-center gap-2">
                    <Led tone="warn" />
                    GitHub Release
                  </span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.dim} className="align-top">
                  <TableCell className="eyebrow pt-4 text-foreground/70">{row.dim}</TableCell>
                  <TableCell className="pt-4 text-[13px] text-muted-foreground">{row.r2}</TableCell>
                  <TableCell className="pt-4 text-[13px] text-muted-foreground">{row.gh}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </Reveal>

      <Reveal delay={120} className="mt-6 grid gap-4 lg:grid-cols-3">
        <div className="panel p-5">
          <span className="eyebrow text-primary">清单</span>
          <p className="mt-2.5 text-[13px] text-muted-foreground">
            客户端读的是 Tauri 更新器格式的 <code className="num text-[12px]">latest.json</code>：
            版本号、发布日期、各平台地址与签名。自建源上这份清单必须随每次发布覆盖。
          </p>
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <CopyButton text={R2_MANIFEST_URL} label="复制清单地址" />
            <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
              <a href={R2_MANIFEST_URL} target="_blank" rel="noreferrer noopener">
                打开
                <ExternalLinkIcon className="size-3.5" />
              </a>
            </Button>
          </div>
        </div>

        <div className="panel p-5">
          <span className="eyebrow text-primary">缓存</span>
          <p className="mt-2.5 text-[13px] text-muted-foreground">
            安装包对象用长缓存（<code className="num text-[12px]">immutable</code>），
            靠地址里的 <code className="num text-[12px]">?v=版本</code> 当指纹：
            换版本就换 URL，CDN 不可能把旧包喂给客户端；清单本身则明确不缓存。
          </p>
        </div>

        <div className="panel p-5">
          <span className="eyebrow text-primary">怎么切</span>
          <p className="mt-2.5 text-[13px] text-muted-foreground">
            客户端：设置 → 更新源（默认自建源，可切 GitHub 逃生）。
            <br />
            本页：顶部切换器，或用参数深链{" "}
            <a
              href="?source=github"
              className="font-mono text-[12px] text-foreground underline decoration-border decoration-dotted underline-offset-4"
            >
              ?source=github
            </a>
            ；选择会记住。
          </p>
          <Button asChild variant="ghost" size="sm" className="mt-4 -ml-1.5 text-muted-foreground">
            <a href={`${REPO_URL}/blob/main/docs/RELEASING.md`} target="_blank" rel="noreferrer noopener">
              发布流程文档
              <ExternalLinkIcon className="size-3.5" />
            </a>
          </Button>
        </div>
      </Reveal>
    </section>
  );
}
