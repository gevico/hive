# Hive — 多代理编排插件设计方案

## 1. 项目概述

### 1.1 定位

Hive 是一个 **Agent 工具无关的多代理编排框架**，面向团队协作场景。它融合了 AgentX 的三层架构（分解、隔离、并行）和 Humanize 的 RLCR 质量循环。核心是独立的 Rust CLI，通过薄适配层嵌入各种 AI 编码工具（Claude Code、Codex CLI、OpenCode 等），实现"Hive 负责编排调度，Agent 工具负责实现质量"的协作模式。

### 1.2 核心理念

- **约束层硬编码**：所有角色限制、状态转换规则、隔离边界用 Rust 实现，不依赖自然语言提示词
- **蜂巢式协作**：每个代理在独立的 worktree（蜂室）中自由工作，编排器（蜂后）只做调度和验收
- **冲突不是错误**：并行开发必然产生冲突，Hive 的职责是按正确顺序合并并尽量自动解决
- **Agent 工具无关**：Hive 只通过 CLI 接口和文件系统通信，不绑定任何特定 AI 编码工具

### 1.3 技术选型

- **实现语言**：Rust，编译为单二进制分发
- **架构**：独立 Rust CLI 为核心，针对不同 agent 工具提供薄适配层（Claude Code skill、Codex hook 等）
- **质量保证**：可插拔质量循环（Humanize RLCR、Codex 内置审查、自定义等）
- **模型策略**：可插拔模型层，默认 Claude，可配置其他模型
- **不包含**：可视化仪表盘（不需要 viz）

---

## 2. 三层架构

对齐 AgentX 的三层分离设计：

```
Layer 0: 编排器 (Rust CLI — hive)
├─ 交互式需求收集（参考 superpowers brainstorming 流程）
├─ 收敛式计划生成（多模型方案收敛）
├─ 任务分解 + 依赖图
├─ 状态机、审计、模型路由
⛔ 绝对禁止编码、编辑文件、操作 worktree 内容

Layer 1: 子代理层 (Rust CLI 硬编码命令)
├─ hive claim    — 领取任务
├─ hive isolate  — 创建 worktree
├─ hive launch   — 启动 worker agent
├─ hive check    — 最终验收验证
├─ hive report   — 上报结构化结果
⛔ 硬编码约束层，非自然语言

Layer 2: 实现层 (Agent tool in worktree — Claude Code / Codex / OpenCode / 其他)
├─ 写代码、跑测试、git commit
├─ 通过可配置的质量循环保证实现质量（如 humanize RLCR、Codex 内置审查等）
├─ worktree 内完全自由（完全执行权限）
⛔ 不能跨 worktree，不能碰主分支
```

### 2.1 层级职责边界

| 层级 | 能做什么 | 不能做什么 |
|------|---------|-----------|
| Layer 0 编排器 | 规划、分解、调度、审计、RLCR 轮次控制、模型路由 | 写代码、改文件、操作任何 worktree 内容 |
| Layer 1 子代理 | 创建/销毁 worktree、启动/停止 agent、验收验证、上报结果 | 绕过状态机、跳跃状态转换 |
| Layer 2 实现者 | 在自己的 worktree 内完全自由（读写文件、执行命令、git commit、跑测试、安装依赖等），可以是任何 agent 工具 | 访问其他 agent 的 worktree、操作主分支 |

### 2.2 Hive 与 Agent 工具的职责分工

| 阶段 | Hive (Layer 0/1) | Agent 工具 (Layer 2) |
|------|------------------|---------------------|
| 需求 → 设计 → 分解 | ✓ | — |
| spec → plan | 提供输入 | 可配置的 plan 生成（如 humanize `gen-plan`） |
| plan → 实现 | 调度、启动 agent | 可配置的质量循环（如 humanize RLCR、Codex 内置审查） |
| 过程审查 | — | Agent 工具内部处理 |
| 最终验收 | `hive check` 验证验收标准 | — |
| 合并 | `hive merge` | — |

### 2.3 Agent 工具适配

Hive 通过 `launch` 配置支持不同的 agent 工具：

| Agent 工具 | launch.tool | 质量循环 | 适配方式 |
|-----------|-------------|---------|---------|
| Claude Code | `claude` | humanize RLCR | Claude Code skill |
| Codex CLI | `codex` | Codex 内置审查 | Codex hook |
| OpenCode | `opencode` | 自定义 | CLI 调用 |
| 自定义 | `custom` | 自定义 | 用户提供启动命令 |

---

## 3. 代理间通信

通过 **Git + 结构化 Markdown 文件** 做共享状态通信，不使用消息队列或直接对话。

所有通信载体使用 **Markdown + YAML frontmatter** 格式，对代理和人类都友好。Rust CLI 解析 frontmatter 中的结构化字段做状态机控制，正文部分供代理和人类阅读。

### 3.1 通信方向

| 通信方向 | 机制 |
|---------|------|
| Layer 0 → Layer 1 | Rust CLI 写入 `.hive/tasks/<task_id>/spec.md`，包含验收标准、上下文文件列表、依赖关系 |
| Layer 1 → Layer 2 | `hive launch` 通过 CLI 参数传入 `--task <task_id>`，agent 从 `specs/<id>.md` + `tasks/<id>/plan.md` 读取任务规格 |
| Layer 2 → Layer 1 | Worker 完成后写入 `.hive/tasks/<task_id>/result.md`，git commit 到 worktree 分支 |
| Layer 1 → Layer 0 | `hive report` 读取 result.md，更新全局 `state.md` + 任务 `audit.md` |
| 失败上报 | Worker 写 result.md 标记 `failed` + 原因，Layer 1 上报给 Layer 0 决策重试/跳过/人工介入 |

**无直接 agent 间通信** — 所有交互都经过文件系统 + Git，编排器是唯一的协调点。

---

## 4. Task ID 设计

### 4.1 命名规则

格式：`<user_name>-<content_hash>`

- `user_name`：从 `git config user.email` 自动提取 `@` 前的部分，可在 `config.local.yml` 中覆盖
- `content_hash`：spec 内容的 `sha256[:8]`，由 `hive plan` 创建任务时自动计算

示例：
```
chao-a1b2c3d4
chao-f5e6d7c8
liyi-9a8b7c6d
```

### 4.2 唯一性保证

- 内容哈希确保同一用户下不同任务不重复
- 用户名前缀确保跨协作者不冲突
- 相同内容产生相同哈希，CLI 检测到已存在则跳过或提示

### 4.3 分支命名

每个任务对应的 git 分支：`hive/<task_id>`

示例：`hive/chao-a1b2c3d4`

---

## 5. 目录结构

```
.hive/
├── config.yml                       # 全局配置（提交）
├── config.local.yml                 # 个人配置（gitignore）
├── state.md                         # 全局任务状态表（gitignore）
│
├── specs/                           # 提交 — 任务契约（RFC 阶段提交）
│   ├── chao-a1b2c3d4.md             # Task: auth middleware
│   └── chao-f5e6d7c8.md             # Task: route handlers
│
├── reports/                         # 提交 — 审计报告（完成后生成）
│   ├── chao-a1b2c3d4.md             # Draft: user auth system
│   └── liyi-9a8b7c6d.md             # Draft: logging pipeline
│
├── plans/                           # gitignore — 决策过程
│   ├── chao-a1b2c3d4/               # Draft: user auth system
│   │   ├── requirements.md
│   │   └── convergence.md
│   └── ...
│
├── tasks/                           # gitignore — 工作文件
│   ├── chao-a1b2c3d4/               # Task: auth middleware
│   │   ├── plan.md                  # 实施步骤
│   │   ├── result.md                # 执行结果
│   │   └── audit.md                 # 审计日志
│   └── ...
│
└── worktrees/                       # gitignore — 临时路径
```

三个顶层目录各司其职：

| 目录 | 提交 | 内容 | 生命周期 |
|------|------|------|---------|
| `specs/` | ✓ | 做什么（验收标准、依赖、复杂度） | RFC 阶段创建 |
| `reports/` | ✓ | 做了什么（per-draft 聚合审计报告） | 完成后生成 |
| `plans/` | ✗ | 为什么这样做（需求澄清、决策过程） | 规划阶段 |
| `tasks/` | ✗ | 怎么做 + 过程（plan、result、audit） | 执行阶段 |

### 5.1 文件提交规则

| 文件 | 提交 | 说明 |
|------|------|------|
| `config.yml` | ✓ | 团队共享配置 |
| `config.local.yml` | ✗ | 个人配置 |
| `specs/*.md` | ✓ | 任务契约（RFC 审查对象） |
| `reports/*.md` | ✓ | 审计报告（完成证明） |
| `state.md` | ✗ | 运行时状态 |
| `plans/` | ✗ | 决策过程，本地参考 |
| `tasks/` | ✗ | 工作文件，聚合到 report |
| `worktrees/` | ✗ | 临时路径 |

仓库里只留**契约（specs）和证明（reports）**。

### 5.2 .gitignore

```gitignore
config.local.yml
state.md
plans/
tasks/
worktrees/
```

---

## 6. 配置系统

### 6.1 双层配置

`hive init` 自动创建以下两个配置文件。

```yaml
# .hive/config.yml (global, committed to repo)

# Audit level: minimal | standard | full
audit_level: standard

merge:
  # Conflict resolution: auto | manual
  conflict_strategy: auto
  # Merge mode: direct | pr
  mode: pr
  # Rebase task branch onto main before merge
  rebase_before_merge: true

# Max retry attempts before marking task as blocked
retry_limit: 3

# Agent tool and quality loop configuration
launch:
  # Agent tool: claude | codex | opencode | custom
  tool: claude
  # Quality loop: humanize | codex-builtin | none
  quality_loop: humanize
  # Custom launch command (only used when tool: custom)
  # custom_command: "my-agent --task {task_id} --worktree {worktree_path}"

# Model binding for each role
# Format: <agent_tool>-<model>-<version>, e.g. claude-opus-4-6, codex-gpt-5-4
agents:
  # Layer 0: planning and convergence
  planner: claude-opus-4-6         # drives interactive planning (Phase 1-5)
  convergence: codex-gpt-5-4       # second model for plan convergence (Phase 4)
  # Layer 2: implementation and review
  worker: claude-sonnet-4-6        # executes tasks in worktrees
  reviewer: codex-gpt-5-4          # final acceptance review (hive check)
```

```yaml
# .hive/config.local.yml (personal, gitignored)
# Overrides config.yml on a per-field basis.
# Created by `hive init` with values from git config.

user:
  # Auto-populated from: git config user.email (part before @)
  name: zevorn
  # Auto-populated from: git config user.email
  email: chao.liu.zevorn@gmail.com

# Override any global config field, e.g.:
# agents:
#   worker: claude-opus-4-6        # use opus locally instead of sonnet
#   reviewer: codex-gpt-5-4
# launch:
#   tool: codex                    # use codex locally instead of claude
#   quality_loop: codex-builtin
```

### 6.2 合并规则

逐字段深度合并，`config.local.yml` 优先：

```
最终生效配置 = deep_merge(config.yml, config.local.yml)
```

`hive config --show` 查看合并后的实际生效配置，标注每个字段来源：

```
audit_level: standard              (global)
launch.tool: claude                (global)
launch.quality_loop: humanize      (global)
agents.planner: claude-opus-4-6    (global)
agents.convergence: codex-gpt-5-4  (global)
agents.worker: claude-opus-4-6     (local override)
agents.reviewer: codex-gpt-5-4     (global)
```

### 6.3 `hive init` 初始化行为

```
$ hive init
  │
  ├─ 1. 检查当前目录是否为 git 仓库，不是则报错退出
  │
  ├─ 2. 创建 .hive/ 目录结构
  │     mkdir -p .hive/{specs,reports,plans,tasks,skills,worktrees}
  │
  ├─ 3. 生成 .hive/config.yml
  │     写入全局配置模板（含所有选项 + # 注释说明可选值）
  │
  ├─ 4. 生成 .hive/config.local.yml
  │     从 git config 自动填充 user.name 和 user.email
  │     其余字段以注释形式列出供用户按需启用
  │
  ├─ 5. 更新项目根目录 .gitignore
  │     追加以下条目（如不存在）：
  │     .hive/config.local.yml
  │     .hive/state.md
  │     .hive/plans/
  │     .hive/tasks/
  │     .hive/worktrees/
  │
  └─ 6. 输出初始化摘要
        Created: .hive/config.yml
        Created: .hive/config.local.yml (user: chao)
        Updated: .gitignore
        Run `hive doctor` to verify environment.
```

如果 `.hive/` 已存在，`hive init` 不会覆盖现有配置，仅补全缺失的文件和目录。

### 6.4 Agent 命名规则

格式：`<agent工具>-<模型>-<版本>`，版本号之间用 `-` 连接。

示例：
- `claude-opus-4-6`
- `claude-sonnet-4-6`
- `claude-haiku-4-5`
- `codex-gpt-5-4`
- `gemini-2-5-pro`

---

## 7. Skill 系统

Skill 是 Layer 2 实现层的能力扩展。Layer 1（Rust CLI 约束层）不使用 skill——它的行为必须硬编码、确定性、不可绕过。

### 7.1 Skill 分层

```
三层 skill 来源：

1. 仓库私有 skill        .hive/skills/<name>.md         提交到仓库
2. 用户全局 skill        ~/.config/hive/skills/<name>.md  个人本地
3. 系统级 skill（plugin） agent 工具内置（如 humanize、superpowers）
```

目录结构：

```
.hive/skills/                        # 仓库私有 skill（提交）
├── coding-style.md                  # 本项目编码规范
├── db-migration.md                  # 本项目 DB 迁移流程
└── deploy-checklist.md              # 本项目部署检查清单

~/.config/hive/skills/               # 用户全局 skill
├── my-rust-patterns.md
└── review-checklist.md
```

### 7.2 Skill 查找优先级

spec 中引用的 skill 名称按以下顺序查找，先找到的优先：

```
1. .hive/skills/<name>.md            仓库私有（最高优先级）
2. ~/.config/hive/skills/<name>.md    用户全局
3. agent 工具内置 plugin              系统级（如 humanize）
```

仓库私有 skill 可以覆盖同名的全局或系统级 skill，实现项目定制。

### 7.3 Skill 配置

```yaml
# .hive/config.yml

skills:
  # Always loaded for all tasks (unless explicitly excluded in spec)
  default:
    - coding-style                   # repo: .hive/skills/coding-style.md

  # Available but only loaded when declared in spec
  available:
    - humanize                       # system plugin
    - db-migration                   # repo: .hive/skills/db-migration.md
    - deploy-checklist               # repo: .hive/skills/deploy-checklist.md
```

- `default`：所有任务自动加载（除非 spec 中用 `exclude_skills` 排除）
- `available`：声明可用 skill 列表，仅在 spec 中显式引用时加载

### 7.4 Spec 中声明 Skill

```markdown
# .hive/specs/chao-a1b2c3d4.md
---
id: chao-a1b2c3d4
skills:
  - humanize                         # 系统级 — RLCR 质量循环
  - db-migration                     # 仓库私有 — DB 迁移流程
exclude_skills:
  - coding-style                     # 排除默认 skill（该任务不需要）
---
```

`hive launch` 根据 spec 计算最终 skill 列表：

```
最终 skill = (config.default - spec.exclude_skills) + spec.skills
```

只加载最终列表中的 skill，worker agent 的上下文保持精简。

### 7.5 `hive launch` 加载行为

```bash
# 根据 spec 中的 skills 字段，只加载指定 skill
# tool: claude
claude --plugin humanize \
       --skill .hive/skills/db-migration.md \
       --agent-prompt "..."

# tool: codex
codex --prompt "$(cat .hive/tasks/chao-a1b2c3d4/plan.md)" \
      --instructions "$(cat .hive/skills/db-migration.md)"
```

### 7.6 Skill 管理命令

```
hive skill <subcommand>
```

| 命令 | 作用 |
|------|------|
| `hive skill list` | 列出所有可用 skill（仓库 + 全局 + 系统级），标注来源和加载状态 |
| `hive skill add <name> [--global]` | 创建空 skill 模板，默认到 `.hive/skills/`，`--global` 到 `~/.config/hive/skills/` |
| `hive skill remove <name> [--global]` | 删除指定 skill 文件 |
| `hive skill show <name>` | 查看 skill 内容和解析来源 |
| `hive skill install <url\|path>` | 从远程 URL 或本地路径安装 skill |
| `hive skill uninstall <name> [--global]` | 卸载已安装的 skill |

### 7.7 安装与卸载流程

**从远程安装（URL/Git）：**

```
$ hive skill install https://github.com/user/repo/skills/tdd.md
  │
  ├─ 1. 下载 skill 文件
  ├─ 2. 验证格式（必须包含 YAML frontmatter: name, description）
  ├─ 3. 复制到 .hive/skills/tdd.md（默认仓库级）
  │     或 ~/.config/hive/skills/tdd.md（加 --global）
  └─ 4. 输出：Installed skill: tdd (repo)

$ hive skill install https://github.com/user/repo/skills/tdd.md --global
  └─ 安装到 ~/.config/hive/skills/tdd.md（全局）
```

**从本地路径安装：**

```
$ hive skill install ~/my-skills/code-review.md
  └─ 复制到 .hive/skills/code-review.md
```

**批量安装（skill pack）：**

```
$ hive skill install https://github.com/user/skill-pack
  │
  ├─ 1. clone 仓库到临时目录
  ├─ 2. 扫描 skills/ 目录下所有 .md 文件
  ├─ 3. 逐个验证并复制到 .hive/skills/
  └─ 4. 输出安装清单
```

**卸载：**

```
$ hive skill uninstall tdd
  │
  ├─ 1. 查找 .hive/skills/tdd.md
  ├─ 2. 检查是否被任何 spec 或 config.yml 引用
  │     ├─ 有引用 → 警告并要求 --force
  │     └─ 无引用 → 直接删除
  └─ 3. 输出：Uninstalled skill: tdd

$ hive skill uninstall tdd --global
  └─ 删除 ~/.config/hive/skills/tdd.md
```

**创建自定义 skill：**

```
$ hive skill add my-convention
  │
  ├─ 1. 创建 .hive/skills/my-convention.md 模板：
  │     ---
  │     name: my-convention
  │     description: ""
  │     ---
  │
  │     (write your skill content here)
  │
  └─ 2. 输出：Created skill template: .hive/skills/my-convention.md
```

### 7.8 Skill 文件格式

```markdown
---
name: db-migration
description: Database migration workflow for this project
---

## Rules

- Always create a reversible migration
- Test migration on a copy of production schema before applying
- ...
```

Rust CLI 在安装时验证 frontmatter 必须包含 `name` 和 `description` 字段。

---

## 8. 任务状态机

### 8.1 任务执行状态 (status)

```
                    ┌──────────┐
                    │ pending  │
                    └────┬─────┘
                         │ hive claim
                         ▼
                    ┌──────────┐
                    │ assigned │
                    └────┬─────┘
                         │ hive isolate + hive launch
                         ▼
                    ┌──────────────┐
              ┌────►│ in_progress  │
              │     └──┬───┬────┬──┘
              │        │   │    │
              │  成功   │   │    │ 失败
              │        │   │    ▼
              │        │   │  ┌────────┐
              │        │   │  │ failed │
              │        │   │  └───┬────┘
     hive     │        │   │      │
     resume   │        │   │ hive pause
              │        │   ▼      │
              │        │ ┌────────┐│
              └────────┤ │ paused ││
                       │ └────────┘│
                       ▼           │ 编排器决策
                 ┌────────┐        ▼
                 │ review │  ┌───────┐ ┌─────────┐
                 └───┬────┘  │ retry │ │ blocked │
                     │       └───┬───┘ └────┬────┘
            验收通过  │           │          │
                     ▼           │          │ 人工介入
              ┌───────────┐      │          │
              │ completed │      └──► pending ◄──┘
              └───────────┘
```

### 8.2 状态转换规则（Rust 硬编码）

| 当前状态 | 可转换到 | 触发条件 |
|---------|---------|---------|
| pending | assigned | `hive claim`，且所有 `depends_on` 任务已 completed |
| assigned | in_progress | `hive isolate` 创建 worktree + `hive launch` 启动 agent |
| in_progress | paused | `hive pause`，agent 写入 checkpoint 后退出 |
| in_progress | review | Worker 写入 result.md，status: completed |
| in_progress | failed | Worker 写入 result.md，status: failed，或超时 |
| paused | in_progress | `hive resume`，从 checkpoint 恢复 |
| review | completed | `hive check` 验证验收标准全部通过 |
| review | failed | `hive check` 验证不通过 |
| failed | retry → pending | 编排器决定重试（重置任务，清理 worktree） |
| failed | blocked | 需要人工介入或依赖外部条件 |
| blocked | pending | 人工解决后释放 |

### 8.3 硬性约束

- 不可跳跃状态（如 pending 不能直接到 review）
- retry 有上限（默认 3 次，可配置），超出自动转 blocked
- 依赖未满足的任务不能被 claim
- 同一任务同一时刻只有一个 agent 持有
- paused 状态的任务保留 worktree 和 checkpoint，不清理

### 8.4 Checkpoint 机制

Agent 在执行过程中周期性写入 checkpoint（每完成一个 plan step、每完成一轮 RLCR），实现任意中断和恢复。

```markdown
# .hive/tasks/chao-a1b2c3d4/checkpoint.md
---
task_id: chao-a1b2c3d4
status: paused
paused_at: 2026-04-13 11:23
last_commit: b3c4d5e
plan_step: 3/7
rlcr_round: 2/5
---

## Progress
- [x] Step 1: Create middleware module
- [x] Step 2: Implement JWT validation
- [x] Step 3: Add error handling (in progress, 60%)
- [ ] Step 4: Route integration
- [ ] Step 5: Unit tests
- [ ] Step 6: Integration tests
- [ ] Step 7: Documentation

## Uncommitted Work
- src/middleware/auth.rs (modified, not committed)

## Resume Instructions
Continue from Step 3: error handling for edge cases.
The JWT validation is complete and committed.
```

### 8.5 Pause 流程

```
hive pause --task chao-a1b2c3d4
    │
    ├─ 1. 发送 SIGTERM 给 worktree 中的 agent 进程
    ├─ 2. Agent 收到信号后：
    │     ├─ 将当前进度写入 checkpoint.md
    │     ├─ git commit 当前工作（如有未提交变更，commit message 标记 [hive:paused]）
    │     └─ 优雅退出
    ├─ 3. Rust CLI 更新状态：in_progress → paused
    ├─ 4. 追加 audit.md：paused at ...
    └─ 5. Worktree 保留不清理

hive pause --all
    └─ 暂停所有 in_progress 任务
```

### 8.6 Resume 流程

```
hive resume --task chao-a1b2c3d4
    │
    ├─ 1. 读取 checkpoint.md 获取断点信息
    ├─ 2. 更新状态：paused → in_progress
    ├─ 3. 重新启动 agent，注入 checkpoint 上下文：
    │     "Resume from Step 3. Steps 1-2 completed.
    │      Last commit: b3c4d5e. See checkpoint.md for details."
    ├─ 4. 追加 audit.md：resumed at ...
    └─ 5. Agent 在同一 worktree 继续工作

hive resume --all
    └─ 恢复所有 paused 任务
```

### 8.7 异常中断恢复

如果 agent 进程被 kill（非优雅退出，无 checkpoint）：

```
hive resume --task chao-a1b2c3d4
    │
    ├─ 1. 检测到 checkpoint.md 不存在或已过期
    ├─ 2. 从 worktree 的 git log 推断进度
    │     （最后一次 commit 对应哪个 plan step）
    ├─ 3. 自动生成 checkpoint.md
    ├─ 4. 正常 resume 流程
    └─ 5. audit.md 标记：recovered from crash
```

### 8.8 计划审批状态 (plan_status)

```
draft → rfc → approved → executing → done
```

- `draft`：`hive plan` 生成中/刚生成
- `rfc`：`hive rfc` 已提交 PR 等待团队审查
- `approved`：PR 通过或用户直接批准
- `executing`：`hive exec` 正在执行
- `done`：执行完成

只有 `plan_status: approved` 的任务才允许被 `hive exec` 调度执行。

---

## 9. 计划生成流程

参考 superpowers brainstorming 的结构化设计流程，分 7 个阶段。

`hive plan` 的交互通过**状态机驱动的对话流程**实现。Rust CLI 不自己做问答，而是作为状态机后端，与前端 agent 工具配合：

```
Agent 工具（Claude Code / Codex / ...）
       ↕ 对话界面
     用户
       ↕ CLI 调用
  hive plan CLI（Rust 状态机）
       ↕ 文件读写
  .hive/plans/<draft_id>/
```

- `hive plan next` — 返回当前 phase 和下一步动作（该问什么问题、该生成什么方案）
- `hive plan answer --draft <id> --phase <n> --response "..."` — 提交用户回答，推进状态机
- `hive plan status --draft <id>` — 查看当前 draft 的进度

这使得 Hive 的计划流程可以嵌入任何支持对话的 agent 工具，不绑定特定平台。

每次 `hive plan` 创建一个独立的 draft（`<user>-<content_hash>`），不同需求各自独立。

### Phase 1: 探索上下文

- 自动扫描代码库结构
- 读取现有文档、最近 commit
- 生成项目现状摘要
- 产出：内部使用，不持久化

### Phase 2: 交互式澄清

- 逐个提问（≥3 个问题）
- 优先多选题，降低用户负担
- 追问直到需求无歧义
- 产出：`.hive/plans/<draft_id>/requirements.md`（gitignore，本地决策参考）

### Phase 3: 提出 2-3 种方案

- 每种方案给出权衡分析
- 明确推荐一种并说明理由
- 用户选择或要求修改

### Phase 4: 收敛式设计

- 多模型独立生成设计方案
- 标记共识 / 分歧
- 收敛轮次（最多 3 轮）
- 收敛条件：
  - 分歧数 = 0 → 自动收敛
  - 连续 2 轮分歧不减少 → 提交用户裁决
  - 达到最大轮次 → 提交用户裁决
- 逐段呈现设计，每段确认
- 产出：`.hive/plans/<draft_id>/convergence.md`（gitignore，本地决策参考）

### Phase 5: 自审

- 占位符扫描（TBD/TODO）
- 内部一致性检查
- 歧义检查
- 范围检查（是否需要进一步分解）
- 自动修复发现的问题

### Phase 6: 任务分解 + Plan 生成

分两步：

1. **Hive 分解**：将收敛后的设计分解为 task 列表，每个 task 生成 `.hive/specs/<id>.md`（目标、验收标准、上下文文件、复杂度、依赖关系）
2. **Plan 生成**：对每个 task，将 spec 作为输入调用可配置的 plan 生成工具（如 `humanize:gen-plan`）生成 `.hive/tasks/<id>/plan.md`

产出：`.hive/specs/<id>.md`（提交）+ `.hive/tasks/<id>/plan.md`（gitignore）

### Phase 7: 用户审批

- 展示完整任务列表 + 依赖图
- 用户可调整复杂度、RLCR 轮次、依赖关系
- 批准 → `plan_status: approved`
- 或 `hive rfc` 进入团队 PR 审查流程

---

## 10. 任务文件格式

### 9.1 specs/<id>.md

```markdown
# .hive/specs/chao-a1b2c3d4.md
---
id: chao-a1b2c3d4
draft_id: chao-b7c8d9e0
status: pending
plan_status: draft
depends_on: []
complexity: M
rlcr_max_rounds: 5
---

## Goal
Implement user authentication middleware

## Acceptance Criteria
- [ ] JWT token validation passes all edge cases
- [ ] Middleware integrates with existing route handler
- [ ] Unit tests cover token expiry, invalid signature, missing header

## Context Files
- src/middleware/mod.rs
- src/routes/auth.rs
```

### 9.2 tasks/<id>/result.md

```markdown
# .hive/tasks/chao-a1b2c3d4/result.md
---
id: chao-a1b2c3d4
status: completed
branch: hive/chao-a1b2c3d4
commit: a1b2c3d
base_commit: e8f9g0h
---

## Dependencies
- base: main @ e8f9g0h
- depends_on:
  - chao-b2c3d4e5 (merged @ f1g2h3i)
  - chao-c3d4e5f6 (merged @ g2h3i4j)

## Environment
- model: claude-opus-4-6
- reviewer: codex-gpt-5-4
- humanize: v1.16.0

## Summary
Implemented JWT authentication middleware with three validation paths.

## Changes
| File | Action | Lines |
|------|--------|-------|
| src/middleware/auth.rs | new | +87 |
| src/routes/auth.rs | modified | +12 -3 |
| src/middleware/mod.rs | modified | +1 |

## Acceptance Criteria Verification
- [x] JWT token validation passes all edge cases
- [x] Middleware integrates with existing route handler
- [x] Unit tests cover token expiry, invalid signature, missing header

## RLCR Summary
- Rounds: 3 / 5 (max)
- Round 1: 2 issues (P1: missing error handling, P2: naming)
- Round 2: 1 issue (P2: edge case test)
- Round 3: 0 issues, clean

## Test Results
- Passed: 12
- Failed: 0
- Skipped: 0

## Notes
Reused existing `TokenValidator` trait, added `JwtValidator` implementation.
```

### 9.3 state.md

```markdown
---
project: user-auth-system
created: 2026-04-13
audit_level: standard
---

## Tasks
| ID | Status | Plan Status | Depends | Assignee | Branch |
|----|--------|-------------|---------|----------|--------|
| chao-a1b2c3d4 | completed | done | — | worker-1 | hive/chao-a1b2c3d4 |
| chao-f5e6d7c8 | in_progress | executing | chao-a1b2c3d4 | worker-2 | hive/chao-f5e6d7c8 |
| chao-g6h7i8j9 | pending | approved | chao-a1b2c3d4 | — | — |
```

---

## 11. 复杂度与 RLCR 轮次

`hive plan` 分解任务时自动评估复杂度并推荐 RLCR 轮次，用户审批时可调整。

| 复杂度 | 特征 | 推荐 RLCR 轮次 |
|--------|------|----------------|
| S | 单文件改动，逻辑简单 | 2 |
| M | 2-5 文件，有接口对接 | 5 |
| L | 5+ 文件，跨模块，新架构 | 8 |

---

## 12. 冲突管理策略

### 11.1 核心原则

多个任务修改同一文件是并行开发的正常产物，不是错误。Hive 的职责是按正确顺序合并，尽量自动解决，解决不了的交给人。

### 11.2 依赖图决定合并顺序

`hive plan` 分解任务时根据逻辑依赖关系确定顺序。无依赖的任务可以并行执行，但按依赖顺序串行合并。

### 11.3 合并策略

```
hive merge --task <id>
    │
    ├─ 1. 将 hive/<task_id> 分支 rebase 到当前 main
    │
    ├─ 2. 无冲突 → 自动合并 ✓
    │
    ├─ 3. 有冲突 → 根据 conflict_strategy：
    │      ├─ auto: 启动 agent 解决冲突
    │      │        给它冲突文件 + 两个任务的 spec.md
    │      │        解决后再跑 hive check 验证验收标准
    │      └─ manual: 标记为 blocked，通知人工处理
    │
    └─ 4. 合并完成后，后续待合并任务自动 rebase
```

### 11.4 GitHub 协作模式

每个任务对应一个 PR：

```
main
 ├── hive/chao-a1b2c3d4  →  PR #1（先合并）
 ├── hive/chao-f5e6d7c8  →  PR #2（rebase on main after PR #1）
 └── hive/chao-g6h7i8j9  →  PR #3（rebase on main after PR #1）
```

`hive merge --mode pr` 自动创建 PR 而非直接合并。

### 11.5 配置项

| 场景 | conflict_strategy | mode |
|------|-------------------|------|
| 个人开发 | auto + direct | 快速合并 |
| 团队协作 | auto + pr | 每个任务一个 PR，CI 验证后合并 |
| 高合规 | manual + pr | 冲突必须人工解决 |

---

## 13. RFC 流程

### 12.1 `hive rfc` 命令

```
hive rfc --task <id>
    │
    ├─ 1. 将 .hive/specs/<id>.md 提交到 hive/<task_id> 分支
    ├─ 2. gh pr create --title "RFC: <task goal>" --label rfc
    ├─ 3. plan_status: draft → rfc
    └─ 4. 输出 PR 链接

hive rfc --all
    │
    └─ 对所有 draft 状态的任务批量创建 RFC PR
```

RFC 只提交 spec（做什么），不提交 plan（怎么做）。团队审查的是目标和验收标准，实施细节由执行者决定。

### 12.2 Plan 状态流转

```
draft       hive plan 生成完成
  │
  ├─ hive rfc → rfc（团队审查）→ PR approved → approved
  │
  └─ 用户直接批准 → approved
        │
        ├─ hive exec → executing
        │
        └─ 完成 → done
```

---

## 14. 审计系统

### 13.1 三档审计等级

| 等级 | 记录内容 | 适用场景 |
|------|---------|---------|
| minimal | 任务状态变更、最终结果、merge 记录 | 个人项目、快速迭代 |
| standard | minimal + 每轮 RLCR 摘要、收敛过程、重试原因 | 日常团队开发 |
| full | standard + 每次代理决策理由、完整 prompt/response 摘要、diff 逐条追溯 | 合规要求、事后复盘 |

### 13.2 任务级审计日志

每个任务独立一份 `audit.md`，追加写入（不可变）：

```markdown
---
task_id: chao-a1b2c3d4
audit_level: standard
---

## Timeline

### 2026-04-13 10:03 — claimed
Assigned to worker-1

### 2026-04-13 10:03 — worktree created
Branch: hive/chao-a1b2c3d4
Path: .hive/worktrees/chao-a1b2c3d4
Base commit: e8f9g0h

### 2026-04-13 10:04 — agent launched
Model: claude-opus-4-6
Context: src/middleware/mod.rs, src/routes/auth.rs

### 2026-04-13 10:31 — RLCR round 1 complete
Commits: 3
Review issues: 2 (P1: missing error handling, P2: naming convention)

### 2026-04-13 10:45 — RLCR round 2 complete
Review issues: 0, all resolved

### 2026-04-13 10:46 — result submitted
Status: completed, Commit: a1b2c3d

### 2026-04-13 10:47 — verification passed
Acceptance criteria: 3/3 ✓

### 2026-04-13 10:48 — merged
Merged to main, commit: e4f5g6h
```

### 13.3 最终审计报告

某个 draft 的所有任务完成后，`hive audit --draft <id>` 聚合生成 per-draft 报告：

```markdown
# .hive/reports/chao-b7c8d9e0.md
---
draft_id: chao-b7c8d9e0
project: user-auth-system
started: 2026-04-13 09:30
completed: 2026-04-13 14:22
audit_level: standard
---

## Overview
- Total tasks: 8
- Completed: 7
- Failed → retried: 1 (chao-e4f5g6h7, retry 1x succeeded)
- Blocked: 0
- Total RLCR rounds: 18
- Human interventions: 2 (plan approval, retry decision)

## Plan Convergence
- Models: claude-opus-4-6, codex-gpt-5-4
- Convergence rounds: 2
- Disagreements resolved: 3 (auto), 1 (user decision)

## Task Execution Summary
| ID | Goal | Status | RLCR Rounds | Duration | Worker |
|----|------|--------|-------------|----------|--------|
| chao-a1b2c3d4 | Auth middleware | ✓ | 2 | 44min | worker-1 |
| chao-f5e6d7c8 | Route handlers | ✓ | 3 | 52min | worker-2 |
| chao-g6h7i8j9 | DB migration | ✓ | 1 | 18min | worker-1 |

## Merge History
| Order | Task | Branch | Commit | Conflicts |
|-------|------|--------|--------|-----------|
| 1 | chao-a1b2c3d4 | hive/chao-a1b2c3d4 | a1b2c3d | none |
| 2 | chao-g6h7i8j9 | hive/chao-g6h7i8j9 | b2c3d4e | none |
| 3 | chao-f5e6d7c8 | hive/chao-f5e6d7c8 | c3d4e5f | 1 file (auto-resolved) |

## Issues & Decisions
1. **chao-e4f5g6h7 failed (round 1)** — OOM during test, retried with reduced batch size
2. **Convergence disagreement #4** — User chose Claude's approach for error handling

## Artifacts
- Task specs: .hive/specs/
- Per-task work files: .hive/tasks/
```

### 13.4 审计写入权限

- Rust CLI（Layer 1）控制写入，agent 不能直接修改审计文件
- 每条记录只追加不修改（append-only）

---

## 15. CLI 命令设计

```
hive <command> [options]
```

### 14.1 Layer 0 — 编排命令（人类直接使用）

| 命令 | 作用 | 阶段 |
|------|------|------|
| `hive init` | 初始化仓库（见 Section 6.3） | 环境准备 |
| `hive config [--show]` | 查看/修改配置（显示合并后生效值及来源） | 环境准备 |
| `hive plan --input <file>` | 启动计划生成流程（7 阶段） | 规划 |
| `hive rfc --task <id> \| --all` | 提交 spec 到仓库并创建 RFC PR | 审查 |
| `hive exec` | 按计划调度执行所有 approved 任务 | 执行 |
| `hive status` | 查看全局状态（任务表 + 进度） | 监控 |
| `hive audit --draft <id>` | 生成 per-draft 审计报告到 `.hive/reports/` | 报告 |
| `hive merge --task <id> \| --all` | 合并已完成任务（支持 `--mode pr`） | 集成 |
| `hive pause --task <id> \| --all` | 暂停任务，写入 checkpoint，保留 worktree（见 Section 8.5） | 控制 |
| `hive resume --task <id> \| --all` | 从 checkpoint 恢复执行（见 Section 8.6） | 控制 |
| `hive abort` | 强制终止所有运行中的 agent，保留 worktree 供复盘 | 应急 |
| `hive skill <sub>` | Skill 管理（list/add/remove/install/uninstall/show，见 Section 7.6） | 管理 |
| `hive doctor` | 检查环境（git、agent 工具、skill、配置的模型等） | 诊断 |

### 14.2 Layer 1 — 子代理命令（由 `hive exec` 内部调用）

| 命令 | 作用 |
|------|------|
| `hive claim --task <id>` | 领取任务，pending → assigned |
| `hive isolate --task <id>` | 创建 worktree + 任务分支，记录 base_commit |
| `hive launch --task <id>` | 在 worktree 中启动配置的 agent 工具 + 质量循环 |
| `hive check --task <id>` | 最终验收：对照 `specs/<id>.md` 验收标准逐项验证 |
| `hive report --task <id>` | 读取 result.md，更新 state.md + audit.md |
| `hive retry --task <id>` | 清理 worktree，重置任务为 pending |
| `hive cleanup --task <id>` | 删除已合并任务的 worktree |

### 14.3 辅助命令

| 命令 | 作用 |
|------|------|
| `hive list-tasks [--status <s>]` | 列出任务（可按状态过滤） |
| `hive show --task <id>` | 查看任务详情（spec + plan + result + audit） |
| `hive graph` | 输出任务依赖图（文本格式） |

### 14.4 典型执行流程

```bash
$ hive init
$ hive config --audit standard

$ hive plan --input feature-spec.md
  # Phase 1-7: 探索 → 澄清 → 方案 → 收敛 → 自审 → 分解(gen-plan) → 审批

$ hive rfc --all
  # 为所有任务创建 RFC PR，团队审查

$ hive exec
  # 自动调度：
  #   1. 扫描 approved + pending 任务，按依赖图确定可并行任务
  #   2. hive claim → hive isolate → hive launch
  #   3. agent 在 worktree 内用 humanize RLCR 执行 plan.md
  #   4. agent 完成后写 result.md
  #   5. hive check 最终验收
  #   6. 失败则 hive retry（不超过上限）
  #   7. 依赖解除后调度新任务
  #   8. 所有任务 completed → 提示用户

$ hive status
$ hive merge --all --mode pr
$ hive audit
```

### 14.5 `hive launch` 内部行为

根据 `launch.tool` 配置选择对应的启动方式：

```bash
# tool: claude (with humanize quality loop)
cd .hive/worktrees/chao-a1b2c3d4
cp .hive/tasks/chao-a1b2c3d4/plan.md .humanize/plan.md
claude --agent-prompt "Execute the plan using humanize:start-rlcr-loop --max 5" \
       --plugin humanize

# tool: codex (with codex-builtin quality loop)
cd .hive/worktrees/chao-a1b2c3d4
codex --approval-mode full-auto \
      --prompt "$(cat .hive/tasks/chao-a1b2c3d4/plan.md)"

# tool: custom
cd .hive/worktrees/chao-a1b2c3d4
my-agent --task chao-a1b2c3d4 --worktree .hive/worktrees/chao-a1b2c3d4
```

所有工具的共同约定：agent 完成后将结果写入 `.hive/tasks/<task_id>/result.md`。

---

## 16. 可复现性

每个 task 的 result.md 包含完整的环境快照，确保任何人都可以复现：

- `base_commit`：worktree 创建时 main 的 HEAD
- `depends_on`：依赖任务及其 merge commit
- `Environment`：使用的模型、humanize 版本
- `branch`：任务分支名
- `commit`：最终提交的 SHA

复现步骤：

```bash
git checkout <base_commit>
# cherry-pick 依赖任务的变更
git cherry-pick <依赖任务的 merge commits>
# 用相同的 specs/<id>.md + tasks/<id>/plan.md 重新执行
hive retry --task <id>
```

---

## 17. 执行流程全景图

```
用户输入需求
    │
    ▼
hive plan (Layer 0)
    ├─ Phase 1: 探索上下文
    ├─ Phase 2: 交互式澄清 → requirements.md
    ├─ Phase 3: 提出 2-3 种方案
    ├─ Phase 4: 收敛式设计 → convergence.md
    ├─ Phase 5: 自审
    ├─ Phase 6: 任务分解 → specs/<id>.md
    │           Plan 生成 → tasks/<id>/plan.md
    └─ Phase 7: 用户审批
    │
    ▼
hive rfc (可选，团队审查)
    ├─ 提交 specs/<id>.md
    └─ 创建 RFC PR → 团队 review → approved
    │
    ▼
hive exec (Layer 1 调度)
    ├─ hive claim → hive isolate → hive launch
    │                                   │
    │                                   ▼
    │                          Layer 2: worktree 内
    │                          Agent 工具 + 质量循环
    │                              ├─ 实现代码
    │                              ├─ 过程审查（Agent 工具内部）
    │                              ├─ 修复 → 循环
    │                              └─ 写 result.md
    │                                   │
    │              ◄────────────────────┘
    ├─ hive check (最终验收)
    ├─ hive report (更新状态 + 审计)
    └─ 调度下一批任务
    │
    ▼
hive merge --all (集成)
    ├─ 按依赖顺序 rebase + 合并
    ├─ 冲突自动解决或人工处理
    └─ 每个任务一个 PR（团队模式）
    │
    ▼
hive audit --draft <id> (审计报告)
    └─ 聚合生成 reports/<draft_id>.md
```

---

## 18. Plugin 封装与导出

Hive 核心是 Rust CLI，针对不同 agent 工具提供适配层。优先以 plugin 形式封装（如 Claude Code plugin），不支持 plugin 的 agent 工具退化为 skill 文件导入。

### 18.1 导出的用户命令

以下命令面向用户，需要导出为 plugin command / skill：

| 命令 | 导出名称 | 说明 |
|------|---------|------|
| `hive init` | `hive:init` | 初始化仓库 |
| `hive plan` | `hive:plan` | 启动计划生成流程 |
| `hive rfc` | `hive:rfc` | 提交 spec 创建 RFC PR |
| `hive exec` | `hive:exec` | 调度执行所有 approved 任务 |
| `hive status` | `hive:status` | 查看全局状态 |
| `hive pause` | `hive:pause` | 暂停任务 |
| `hive resume` | `hive:resume` | 恢复任务 |
| `hive merge` | `hive:merge` | 合并已完成任务 |
| `hive audit` | `hive:audit` | 生成审计报告 |
| `hive skill` | `hive:skill` | Skill 管理 |
| `hive doctor` | `hive:doctor` | 环境检查 |
| `hive graph` | `hive:graph` | 任务依赖图 |

### 18.2 不导出的内部命令

以下命令由 `hive exec` 内部调用（Layer 1），不暴露给用户：

```
hive claim / hive isolate / hive launch / hive check
hive report / hive retry / hive cleanup
```

### 18.3 Claude Code Plugin（优先）

完整的 plugin 封装，包含 skills、hooks、agents：

```
.claude-plugin/
├── plugin.json                      # 插件元数据

skills/
├── hive-init/SKILL.md               # /hive:init
├── hive-plan/SKILL.md               # /hive:plan （交互式，状态机驱动）
├── hive-rfc/SKILL.md                # /hive:rfc
├── hive-exec/SKILL.md               # /hive:exec
├── hive-status/SKILL.md             # /hive:status
├── hive-pause/SKILL.md              # /hive:pause
├── hive-resume/SKILL.md             # /hive:resume
├── hive-merge/SKILL.md              # /hive:merge
├── hive-audit/SKILL.md              # /hive:audit
├── hive-skill/SKILL.md              # /hive:skill
├── hive-doctor/SKILL.md             # /hive:doctor
└── hive-graph/SKILL.md              # /hive:graph

hooks/
├── hooks.json
├── hive-orchestrator-guard.sh       # PreToolUse: 阻止 Layer 0 agent 写代码
└── hive-exec-stop-gate.sh           # Stop: 任务未全部完成时阻止退出

agents/
├── hive-planner.md                  # Layer 0 规划代理（禁止编码）
└── hive-worker.md                   # Layer 2 实现代理（worktree 内自由）
```

**plugin.json 示例：**

```json
{
  "name": "hive",
  "description": "Agent-agnostic multi-agent orchestration framework",
  "version": "0.1.0",
  "author": { "name": "hive" },
  "license": "MIT"
}
```

**Skill 示例（hive-plan/SKILL.md）：**

```markdown
---
name: hive:plan
description: Start interactive planning flow for a new feature or requirement
user_invocable: true
---

This skill drives the Hive planning flow (Phase 1-7) through a state-machine
driven conversation. It wraps `hive plan` CLI commands.

## Usage
/hive:plan --input <requirements_file>

## Flow
1. Call `hive plan start --input <file>` to create a new draft
2. Loop: call `hive plan next --draft <id>` to get next action
3. Present question/options to user, collect response
4. Call `hive plan answer --draft <id> --phase <n> --response "..."`
5. Repeat until all phases complete
6. Present task list + dependency graph for user approval
```

**Hook 示例（hive-orchestrator-guard.sh）：**

```bash
#!/bin/bash
# PreToolUse hook: block code-writing tools when running as Layer 0 orchestrator
if [ "$HIVE_ROLE" = "orchestrator" ]; then
    if [[ "$TOOL_NAME" =~ ^(Write|Edit|NotebookEdit)$ ]]; then
        echo '{"result":"block","message":"Layer 0 orchestrator cannot write code"}'
        exit 0
    fi
fi
echo '{"result":"allow"}'
```

### 18.4 Codex CLI 适配

Codex 不支持完整 plugin 体系，使用 instructions 文件和 hook 适配：

```
.codex/
├── instructions.md                  # Codex 系统指令，引导调用 hive CLI
└── hooks.json                       # Codex hook（如有支持）
```

**instructions.md 示例：**

```markdown
You are working in a Hive-managed repository. Use the `hive` CLI for all
orchestration tasks:

- `hive plan --input <file>` to start planning
- `hive exec` to execute approved tasks
- `hive status` to check progress
- `hive pause/resume` to control tasks
- `hive merge --all` to integrate completed work
- `hive audit --draft <id>` to generate reports

Do NOT write code directly in the main workspace. All implementation
must happen through `hive exec` which creates isolated worktrees.
```

### 18.5 通用 Skill 文件适配

对于不支持 plugin 也不支持特定集成格式的 agent 工具（如 OpenCode 等），提供独立的 skill markdown 文件：

```
adapters/
├── claude/                          # Claude Code plugin（完整）
│   ├── .claude-plugin/
│   ├── skills/
│   ├── hooks/
│   └── agents/
├── codex/                           # Codex 适配
│   └── .codex/
└── generic/                         # 通用 skill 文件
    ├── hive-init.md
    ├── hive-plan.md
    ├── hive-exec.md
    ├── hive-status.md
    ├── hive-pause.md
    ├── hive-resume.md
    ├── hive-merge.md
    ├── hive-audit.md
    ├── hive-skill.md
    └── hive-doctor.md
```

通用 skill 文件格式与 Claude Code SKILL.md 相同（YAML frontmatter + markdown），任何支持 markdown skill 的 agent 工具都可以直接加载。

### 18.6 适配层选择

`hive init` 根据检测到的 agent 工具自动安装对应适配层：

```
$ hive init
  │
  ├─ 检测 agent 工具环境
  │   ├─ 发现 claude CLI → 安装 Claude Code plugin 适配
  │   ├─ 发现 codex CLI → 安装 Codex 适配
  │   └─ 其他 → 安装通用 skill 文件
  │
  └─ 输出：
     Detected: claude (Claude Code)
     Installed: Claude Code plugin adapter
     Skills: 12 commands exported as hive:* skills
```

多个 agent 工具共存时，可以同时安装多个适配层。
