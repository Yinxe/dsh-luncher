import { Toaster } from "@/components/ui/sonner";
import { DownloadMatrix } from "@/components/site/download-matrix";
import { Faq } from "@/components/site/faq";
import { Features } from "@/components/site/features";
import { Footer } from "@/components/site/footer";
import { Hero } from "@/components/site/hero";
import { InstallNotes } from "@/components/site/install-notes";
import { ReleaseNotes } from "@/components/site/release-notes";
import { Shot } from "@/components/site/shot";
import { SourceCompare } from "@/components/site/source-compare";
import { TopBar } from "@/components/site/top-bar";
import { useRelease } from "@/hooks/use-release";

/**
 * 页面骨架。
 *
 * 数据只有一个来源（useRelease）：顶部横幅、首屏、下载矩阵、安装命令、更新日志、页脚
 * 全部读同一份状态 —— 所以版本号不可能出现「上面是 0.1.7、下面是 0.1.6」这种自相矛盾。
 */
export default function App() {
  const controller = useRelease();

  return (
    <div className="relative min-h-screen">
      <div className="grain" aria-hidden="true" />

      <div className="relative z-10">
        <TopBar controller={controller} />
        <main>
          <Hero controller={controller} />
          <DownloadMatrix controller={controller} />
          <InstallNotes state={controller.state} />
          <SourceCompare state={controller.state} />
          <Features />
          <Shot />
          <ReleaseNotes state={controller.state} />
          <Faq state={controller.state} />
        </main>
        <Footer state={controller.state} />
      </div>

      <Toaster position="bottom-center" />
    </div>
  );
}
