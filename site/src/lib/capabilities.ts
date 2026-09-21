/*
 * 全部能力清单 —— 按领域分组，逐条对齐 README 的功能段落。
 *
 * 这一份是「规格表」：条目短、密度高、只讲是什么（怎么做的、边界在哪，写在首页的
 * 「核心机制」与常见问题里）。所以改动这里时请同时看 README.md 的「功能」一节，
 * 两处不一致就等于对用户说了两种产品。
 */

export interface Capability {
  title: string;
  detail: string;
}

export interface CapabilityGroup {
  id: string;
  label: string;
  items: Capability[];
}

export const CAPABILITY_GROUPS: CapabilityGroup[] = [
  {
    id: "versions",
    label: "版本与安装",
    items: [
      { title: "官方版本列表", detail: "拉 npm registry 上 dsh 的全部版本，含 latest / next / alpha 渠道、发布日期与体积，可切镜像源。" },
      { title: "已装版本三处识别", detail: "启动器自己的目录、npm 全局、PATH 里的 dsh 一起认（顺符号链接解析出版本）。" },
      { title: "安装 / 卸载 / 重装", detail: "每个版本装进独立目录，各版本互不干扰，也不动全局 npm 前缀。" },
      { title: "安装过程可见可取消", detail: "fetch / resolve / extract 逐条实时打印，装到一半可以取消。" },
      { title: "逐版本看官方更新日志", detail: "内置版本浏览器：按版本翻 Release 正文，官方的中英双段可切，可跳完整提交对比。" },
      { title: "首次使用引导", detail: "全新机器上一键完成 dsh 初始化，profile 列表与默认 profile 自动就位。" },
    ],
  },
  {
    id: "instances",
    label: "实例与终端",
    items: [
      { title: "内嵌启动", detail: "dsh 作为启动器的子进程跑，日志逐行进底部面板：多实例 tab、运行时长、一键停止。" },
      { title: "终端启动", detail: "要在交互式 TUI 里跑，就在独立系统终端里拉起（自动探测 gnome-terminal / konsole / kitty / Windows Terminal 等）。" },
      { title: "每 profile 一个实例", detail: "两种启动方式受同一约束，不会跑出两份实例去抢同一个端口。" },
      { title: "退出即回收", detail: "关掉启动器，它拉起的 dsh 子进程随之结束，不留孤儿进程。" },
      { title: "profile 与默认参数", detail: "顶栏切换 profile、记住默认 profile，并可设默认附加参数（如 --preset qqbot）。" },
    ],
  },
  {
    id: "plugins",
    label: "插件管理",
    items: [
      { title: "只用官方命令", detail: "安装 / 卸载 / 升级都走 dsh plugin add / remove，从不改写 profile 的 package.json 或 dsh.profile.bundles。" },
      { title: "三种安装方式", detail: "npm 包（registry 搜索或精确规格）、链接直装（link: 路径 / 仓库链接 / .tgz 直链）、clone 仓库 + 本地 link（可自动 pnpm install + build）。" },
      { title: "monorepo 子包探测", detail: "按 pnpm-workspace / workspaces 展开子包，列出名称、版本、是否声明 dsh.bundle、lib/ 是否就绪。" },
      { title: "装后校验", detail: "缺清单、没有可加载入口、loader entry id 冲突，都会当场卸掉并说明原因。" },
      { title: "卸载保护", detail: "你自己的 cordis.patch.yml 仍引用该包时拒绝卸载并指出要删哪几行；带原生模块会提示需要重启。" },
      { title: "更新检测不花 API 额度", detail: "npm 包比 registry latest，github: 与 clone 源用 git ls-remote；私有仓库标注「不支持」，纯本地 link 与 .tgz 标注「无更新渠道」。" },
      { title: "内置任务终端", detail: "逐行实时输出、按流着色、可复制或导出，一键取消连派生进程一起收。" },
    ],
  },
  {
    id: "config",
    label: "配置与凭据",
    items: [
      { title: "模型配置", detail: "结构化编辑 llm-pi-ai.providers（密钥 / 名称 / 地址 / 协议 / 模型列表）与默认模型的三级联动。" },
      { title: "获取可用模型", detail: "从服务方 GET {baseURL}/models 拉列表勾选添加；密钥按 手动值 > 凭据引用 > 环境变量 解析。" },
      { title: "凭据管理（含注释）", detail: "只管引用名与值，值只在详情里可见；键正上方一行 # 注释就是这条凭据的说明，可就地编辑。" },
      { title: "只改该改的", detail: "保存只重写目标小节，节外注释与未识别字段逐字节保留，写前自动备份。" },
      { title: "内嵌 YAML 编辑器", detail: "CodeMirror 6：语法高亮、行内报错、保留注释，高度自适应。" },
    ],
  },
  {
    id: "network",
    label: "网络与更新",
    items: [
      { title: "两条更新通道", detail: "默认自建源（Cloudflare R2），GitHub Release 兜底，设置里二选一。" },
      { title: "应用内自更新", detail: "读清单 → 验签 → 安装并重启；AppImage 原位替换、deb / rpm 弹授权、Windows 交给安装器。" },
      { title: "更新说明就是 CHANGELOG", detail: "横幅与弹窗直接展示本版正文 —— 发版时 CI 强制要求写清，缺失就发不出去。" },
      { title: "GitHub 加速", detail: "内置常用前缀 + 首次自动测速（下载与 git 分开测），可固定或自建；只注入进程内 git 配置，永不写进 origin。" },
      { title: "通道自检", detail: "一键并发探测 github refs / jsDelivr / raw / api.github.com 的延迟，连续失败的通道临时熔断 5 分钟。" },
      { title: "GitHub Token（可选）", detail: "填了就把握度从 60 次/时提到 5000 次/时，设置页显示剩余额度与重置时间。" },
    ],
  },
  {
    id: "system",
    label: "系统集成",
    items: [
      { title: "托盘常驻", detail: "左键切换窗口，右键菜单列出各 profile 的运行状态。" },
      { title: "启动器单实例", detail: "重复双击只会唤回已有窗口，不会开出第二个托盘图标。" },
      { title: "内置 Node 运行时", detail: "机器上没有 Node 也能一键装进启动器自己的目录（免 root、走国内镜像）。" },
      { title: "分类日志", detail: "9 个子系统分文件、单文件超 1MB 自动滚动，grep ERROR 就能定位。" },
      { title: "诊断包", detail: "环境摘要 + 设置（凭据已脱敏）+ 日志合成一个文件，报 bug 发一个就够。" },
      { title: "平台适配", detail: "Windows 后台命令不弹黑框、npm.cmd 与 os error 193 这类坑已处理；Linux 托盘需要 libayatana-appindicator3。" },
    ],
  },
];
