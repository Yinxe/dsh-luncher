# DSH Launcher — Agent 规则

## UI 规则（硬性）

- **所有 UI 统一使用 shadcn/ui 全套组件与风格**：优先复用 `src/components/ui/` 下已有的组件（Button、Card、Badge、Dialog、Sheet、AlertDialog、Alert、Tabs、ToggleGroup、Table、Select、Switch、Input、Textarea、Label、Field、Progress、Sonner Toast 等）。
- **禁止手写/新建 UI 组件**：除非 shadcn 没有合适的组件，否则不得自建组件、不得用原生标签堆砌已有组件的替代品（如手写 `<table>`、手写弹窗、手写遮罩、手写 toast）。
- 新增组件一律用 CLI：`npx shadcn@latest add <component>`；修改 shadcn 生成的源码前需确认为项目定制（如 sonner 的主题源已改为 `@/lib/theme`）。
- 组件底层 primitive 统一来自合并包 `radix-ui`（shadcn@latest 约定），不要安装/引用 `@radix-ui/react-*` 单包。
- 全局 toast 用 sonner：`import { toast } from "sonner"` + `<Toaster />`（`@/components/ui/sonner`），业务侧统一走 `addToast(kind, text)`。
- 确认类交互用 `AlertDialog`，弹窗用 `Dialog`，侧滑抽屉用 `Sheet`，分段选择用 `Tabs`，多选/单选 chips 用 `ToggleGroup`，数据表用 `Table`。
- 项目自有主题系统在 `src/lib/theme.tsx`（`useTheme` 返回 `{ theme, resolved, setTheme }`），不要引入 next-themes。

## 构建

- 前端：`npm run build`（tsc + vite build）
- 桌面调试包：`npm run build:debug`；发布：`npm run build:release`
