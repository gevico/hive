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
| Layer 1 → Layer 2 | `hive launch` 通过 CLI 参数传入 `--task <task_id>`，agent 从 spec.md + plan.md 读取任务规格 |
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
├── config.local.yml                 # 个人配置（gitignore，hive init 创建）
├── state.md                         # 全局任务状态表（gitignore）
├── report.md                        # 最终审计报告（提交，hive audit 生成）
├── plan/
│   ├── chao-a1b2c3d4/               # Draft: user auth system
│   │   ├── requirements.md          # 该需求的澄清记录
│   │   └── convergence.md           # 该需求的决策过程记录
│   ├── chao-f5e6d7c8/               # Draft: payment integration
│   │   ├── requirements.md
│   │   └── convergence.md
│   └── ...
├── tasks/
│   ├── chao-a1b2c3d4/
│   │   ├── spec.md                  # 任务规格（提交）
│   │   ├── plan.md                  # 实施计划（提交）
│   │   ├── result.md                # 执行结果（提交）
│   │   └── audit.md                 # 审计日志（按审计等级决定）
│   └── ...
└── worktrees/                       # worktree 路径记录（gitignore）
```

### 5.1 文件提交规则

| 文件 | 提交 | 说明 |
|------|------|------|
| `config.yml` | ✓ | 团队共享配置 |
| `config.local.yml` | ✗ | 个人信息和偏好 |
| `state.md` | ✗ | 运行时临时状态 |
| `report.md` | ✓ | 最终审计报告 |
| `tasks/*/spec.md` | ✓ | 任务契约 |
| `tasks/*/plan.md` | ✓ | 实施方案 |
| `tasks/*/result.md` | ✓ | 执行结果 |
| `plan/` | ✗ | 需求澄清和决策过程，本地参考 |
| `tasks/*/audit.md` | 按等级 | minimal/standard 不提交，full 提交 |
| `worktrees/` | ✗ | 临时路径 |

### 5.2 .gitignore

```gitignore
config.local.yml
state.md
plan/
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
  │     mkdir -p .hive/{plan,tasks,worktrees}
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
  │     .hive/plan/
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

## 7. 任务状态机

### 7.1 任务执行状态 (status)

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
                    │ in_progress  │
                    └──┬───────┬───┘
                       │       │
              成功完成  │       │ 失败
                       ▼       ▼
                 ┌────────┐ ┌────────┐
                 │ review │ │ failed │
                 └───┬────┘ └───┬────┘
                     │          │
            验收通过  │          │ 编排器决策
                     ▼          ▼
              ┌───────────┐ ┌───────┐ ┌─────────┐
              │ completed │ │ retry │ │ blocked │
              └───────────┘ └───┬───┘ └────┬────┘
                                │          │
                                │          │ 人工介入解决
                                └──► pending ◄──┘
```

### 7.2 状态转换规则（Rust 硬编码）

| 当前状态 | 可转换到 | 触发条件 |
|---------|---------|---------|
| pending | assigned | `hive claim`，且所有 `depends_on` 任务已 completed |
| assigned | in_progress | `hive isolate` 创建 worktree + `hive launch` 启动 agent |
| in_progress | review | Worker 写入 result.md，status: completed |
| in_progress | failed | Worker 写入 result.md，status: failed，或超时 |
| review | completed | `hive check` 验证验收标准全部通过 |
| review | failed | `hive check` 验证不通过 |
| failed | retry → pending | 编排器决定重试（重置任务，清理 worktree） |
| failed | blocked | 需要人工介入或依赖外部条件 |
| blocked | pending | 人工解决后释放 |

### 7.3 硬性约束

- 不可跳跃状态（如 pending 不能直接到 review）
- retry 有上限（默认 3 次，可配置），超出自动转 blocked
- 依赖未满足的任务不能被 claim
- 同一任务同一时刻只有一个 agent 持有

### 7.4 计划审批状态 (plan_status)

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

## 8. 计划生成流程

参考 superpowers brainstorming 的结构化设计流程，分 7 个阶段。

`hive plan` 的交互通过**状态机驱动的对话流程**实现。Rust CLI 不自己做问答，而是作为状态机后端，与前端 agent 工具配合：

```
Agent 工具（Claude Code / Codex / ...）
       ↕ 对话界面
     用户
       ↕ CLI 调用
  hive plan CLI（Rust 状态机）
       ↕ 文件读写
  .hive/plan/<draft_id>/
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
- 产出：`.hive/plan/<draft_id>/requirements.md`（gitignore，本地决策参考）

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
- 产出：`.hive/plan/<draft_id>/convergence.md`（gitignore，本地决策参考）

### Phase 5: 自审

- 占位符扫描（TBD/TODO）
- 内部一致性检查
- 歧义检查
- 范围检查（是否需要进一步分解）
- 自动修复发现的问题

### Phase 6: 任务分解 + Plan 生成

分两步：

1. **Hive 分解**：将收敛后的设计分解为 task 列表，每个 task 生成 `spec.md`（目标、验收标准、上下文文件、复杂度、依赖关系）
2. **Humanize gen-plan**：对每个 task，将 spec.md 作为输入调用 `humanize:gen-plan` 生成 `plan.md`，确保 100% 符合 humanize 格式，可直接被 `humanize:start-rlcr-loop` 执行

产出：`.hive/tasks/<id>/spec.md` + `plan.md`

### Phase 7: 用户审批

- 展示完整任务列表 + 依赖图
- 用户可调整复杂度、RLCR 轮次、依赖关系
- 批准 → `plan_status: approved`
- 或 `hive rfc` 进入团队 PR 审查流程

---

## 9. 任务文件格式

### 9.1 spec.md

```markdown
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

### 9.2 result.md

```markdown
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

## 10. 复杂度与 RLCR 轮次

`hive plan` 分解任务时自动评估复杂度并推荐 RLCR 轮次，用户审批时可调整。

| 复杂度 | 特征 | 推荐 RLCR 轮次 |
|--------|------|----------------|
| S | 单文件改动，逻辑简单 | 2 |
| M | 2-5 文件，有接口对接 | 5 |
| L | 5+ 文件，跨模块，新架构 | 8 |

---

## 11. 冲突管理策略

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

## 12. RFC 流程

### 12.1 `hive rfc` 命令

```
hive rfc --task <id>
    │
    ├─ 1. 将 spec.md + plan.md 提交到 hive/<task_id> 分支
    ├─ 2. gh pr create --title "RFC: <task goal>" --label rfc
    ├─ 3. plan_status: draft → rfc
    └─ 4. 输出 PR 链接

hive rfc --all
    │
    └─ 对所有 draft 状态的任务批量创建 RFC PR
```

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

## 13. 审计系统

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

所有任务完成后，`hive audit` 聚合生成项目级总报告：

```markdown
---
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
- Per-task details: .hive/tasks/*/
```

### 13.4 审计写入权限

- Rust CLI（Layer 1）控制写入，agent 不能直接修改审计文件
- 每条记录只追加不修改（append-only）

---

## 14. CLI 命令设计

```
hive <command> [options]
```

### 14.1 Layer 0 — 编排命令（人类直接使用）

| 命令 | 作用 | 阶段 |
|------|------|------|
| `hive init` | 初始化仓库（见 Section 6.3） | 环境准备 |
| `hive config [--show]` | 查看/修改配置（显示合并后生效值及来源） | 环境准备 |
| `hive plan --input <file>` | 启动计划生成流程（7 阶段） | 规划 |
| `hive rfc --task <id> \| --all` | 提交 spec+plan 到仓库并创建 RFC PR | 审查 |
| `hive exec` | 按计划调度执行所有 approved 任务 | 执行 |
| `hive status` | 查看全局状态（任务表 + 进度） | 监控 |
| `hive audit` | 生成最终审计报告 `.hive/report.md` | 报告 |
| `hive merge --task <id> \| --all` | 合并已完成任务（支持 `--mode pr`） | 集成 |
| `hive abort` | 终止所有运行中的 agent，保留 worktree 供复盘 | 应急 |
| `hive doctor` | 检查环境（git、claude、humanize、配置的模型等） | 诊断 |

### 14.2 Layer 1 — 子代理命令（由 `hive exec` 内部调用）

| 命令 | 作用 |
|------|------|
| `hive claim --task <id>` | 领取任务，pending → assigned |
| `hive isolate --task <id>` | 创建 worktree + 任务分支，记录 base_commit |
| `hive launch --task <id>` | 在 worktree 中启动 Claude Code agent + humanize RLCR |
| `hive check --task <id>` | 最终验收：对照 spec.md 验收标准逐项验证 |
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

## 15. 可复现性

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
# 用相同的 spec.md + plan.md 重新执行
hive retry --task <id>
```

---

## 16. 执行流程全景图

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
    ├─ Phase 6: 任务分解 → spec.md
    │           调用 humanize:gen-plan → plan.md
    └─ Phase 7: 用户审批
    │
    ▼
hive rfc (可选，团队审查)
    ├─ 提交 spec.md + plan.md
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
hive audit (审计报告)
    └─ 聚合生成 report.md
```
