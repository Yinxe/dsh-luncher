import { useEffect, useState } from "react";
import { ArrowLeft, Loader2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import YamlEditor from "@/components/YamlEditor";
import ProfileFilesTab from "@/components/ProfileFilesTab";
import ProfileQuickTab from "@/components/ProfileQuickTab";
import PluginsTab from "@/components/PluginsTab";
import ModelConfigView from "@/components/ModelConfigView";
import { api } from "../api";
import type { ProfileConfigMode, ProfileTarget } from "../types";

interface Props {
  /** 左列选中的 profile；null = 未选中（显示引导占位） */
  profile: string | null;
  /** 全部 profile 名（模型「同步到其他 profile」勾选清单用） */
  profiles: string[];
  target: ProfileTarget;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  /** 窄屏（工作台占满整页）返回实例列表；宽屏双栏时不显示 */
  onBack?: () => void;
  /** 插件任务流（App 层 usePluginJobs）：透传给「插件」Tab 做 busy 锁/刷新/终端跳转 */
  pluginRunningCount: number;
  pluginJobsTick: number;
  onOpenPluginTerminal: () => void;
}

const TAB_KEYS = ["quick", "models", "plugins", "files", "package"] as const;
type TabKey = (typeof TAB_KEYS)[number];

/** 使用习惯：切到某个 profile 的配置时，默认停在上次离开的 Tab（跨 profile / 重启记忆） */
const TAB_STORAGE_KEY = "dsh-starter:workspace-tab";

function lastUsedTab(fallback: TabKey): TabKey {
  try {
    const s = localStorage.getItem(TAB_STORAGE_KEY);
    return s && (TAB_KEYS as readonly string[]).includes(s) ? (s as TabKey) : fallback;
  } catch {
    return fallback;
  }
}

/** package.json 只读查看：写入归 dsh plugin 命令管（插件 Tab） */
function PackageTab({ profile }: { profile: string }) {
  const [raw, setRaw] = useState<string | null>(null);
  useEffect(() => {
    setRaw(null);
    api.readProfileFile(profile, "package.json")
      .then(setRaw)
      .catch(() => setRaw(""));
  }, [profile]);
  if (raw == null) {
    return (
      <div className="flex items-center justify-center gap-2 py-8 text-xs text-muted-foreground">
        <Loader2 className="h-3.5 w-3.5 animate-spin" /> 读取 package.json…
      </div>
    );
  }
  return (
    <div className="space-y-2">
      <p className="text-[11px] text-muted-foreground">
        只读查看：dsh.profile.bundles 声明插件包与运行形态；安装/卸载请去「插件」Tab（dsh plugin 命令）。
      </p>
      <YamlEditor value={raw} readOnly onChange={() => undefined} />
    </div>
  );
}

/**
 * Profile 工作台（右栏）：「确定 profile → 各种配置」。
 * 配置归属（patch = 0.1.7+ / legacy = 旧版全局）由后端按该 profile 绑定的启动版本判定，
 * 各 Tab 据此读写 cordis.patch.yml 或全局 settings.yaml。Tab 首次打开才挂载，
 * 之后保持挂载（切 Tab 不丢未保存的编辑）。
 */
export default function ProfileWorkspace({
  profile, profiles, target, onToast, onBack,
  pluginRunningCount, pluginJobsTick, onOpenPluginTerminal,
}: Props) {
  const [mode, setMode] = useState<ProfileConfigMode | null>(null);
  const isWeb = target === "web";
  const [tab, setTab] = useState<TabKey>(() => (isWeb ? "quick" : "models"));
  const [visited, setVisited] = useState<Set<TabKey>>(new Set());

  // 换 profile：回到上次使用的 Tab（无记录则第一个），配置归属重新拉
  useEffect(() => {
    if (!profile) return;
    const saved = lastUsedTab(isWeb ? "quick" : "models");
    const initial = saved === "quick" && !isWeb ? "models" : saved;
    setTab(initial);
    setVisited(new Set([initial]));
    setMode(null);
    api.getProfileConfigMode(profile).then(setMode).catch((e) => {
      setMode(null);
      onToast("err", `读取 profile 配置归属失败: ${e}`);
    });
  }, [profile, isWeb, onToast]);

  const openTab = (v: string) => {
    const k = v as TabKey;
    setTab(k);
    setVisited((s) => (s.has(k) ? s : new Set(s).add(k)));
    try {
      localStorage.setItem(TAB_STORAGE_KEY, k);
    } catch {
      // 记不住就算了，不影响切 Tab
    }
  };

  if (!profile) {
    return (
      <Card className="p-10 text-center text-sm text-muted-foreground">
        在左侧点某个 profile 的「配置」按钮，就能在这里维护它的
        快捷配置、模型、插件与 <span className="font-mono">cordis.patch.yml</span>。
      </Card>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        {onBack && (
          <Button
            variant="ghost"
            size="sm"
            className="xl:hidden"
            title="返回实例列表"
            onClick={onBack}
          >
            <ArrowLeft /> 返回
          </Button>
        )}
        <h2 className="font-mono text-base font-semibold">{profile}</h2>
        {mode == null ? (
          <Badge variant="outline" className="gap-1">
            <Loader2 className="h-3 w-3 animate-spin" /> 配置归属…
          </Badge>
        ) : mode.mode === "patch" ? (
          <Badge variant="success" title="0.1.7+：该 profile 的配置独立存于自己的 cordis.patch.yml，与其他 profile 互不影响">
            dsh {mode.version} · 按 profile 独立
          </Badge>
        ) : (
          <Badge variant="warning" title="绑定的 dsh 早于 0.1.7：模型与配置仍读写全局 ~/.dsh/settings.yaml，该能力即将弃用">
            dsh {mode.version || "?"} · 旧版全局配置
          </Badge>
        )}
      </div>

      {!mode ? (
        <div className="flex items-center justify-center gap-2 py-12 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> 读取配置归属…
        </div>
      ) : (
        <Tabs value={tab} onValueChange={openTab} className="gap-3">
          <TabsList>
            {isWeb && <TabsTrigger value="quick" className="text-xs">快捷配置</TabsTrigger>}
            <TabsTrigger value="models" className="text-xs">模型</TabsTrigger>
            <TabsTrigger value="plugins" className="text-xs">插件</TabsTrigger>
            <TabsTrigger value="files" className="text-xs font-mono">配置文件</TabsTrigger>
            <TabsTrigger value="package" className="text-xs font-mono">package.json</TabsTrigger>
          </TabsList>
          {/* 懒挂载 + 保持挂载：首次打开才加载，切走只隐藏（草稿不丢，同凭据页做法；
              Radix Tabs 无 forceMount，面板用受控 div 渲染） */}
          <div className="min-w-0">
            {isWeb && visited.has("quick") && (
              <div className={tab === "quick" ? "" : "hidden"}>
                <ProfileQuickTab profile={profile} onToast={onToast} />
              </div>
            )}
            {visited.has("models") && (
              <div className={tab === "models" ? "" : "hidden"}>
                <ModelConfigView profile={profile} profiles={profiles} onToast={onToast} />
              </div>
            )}
            {visited.has("plugins") && (
              <div className={tab === "plugins" ? "" : "hidden"}>
                <PluginsTab
                  profile={profile}
                  onToast={onToast}
                  pluginRunningCount={pluginRunningCount}
                  pluginJobsTick={pluginJobsTick}
                  onOpenPluginTerminal={onOpenPluginTerminal}
                />
              </div>
            )}
            {visited.has("files") && (
              <div className={tab === "files" ? "" : "hidden"}>
                <ProfileFilesTab profile={profile} mode={mode} onToast={onToast} />
              </div>
            )}
            {visited.has("package") && (
              <div className={tab === "package" ? "" : "hidden"}>
                <PackageTab profile={profile} />
              </div>
            )}
          </div>
        </Tabs>
      )}
    </div>
  );
}
