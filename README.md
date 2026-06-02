# CoffeeTable

> **A note from the author.** CoffeeTable is an opinionated IDE tailored to my
> individual needs. The main motivation is shaping a development environment
> that fits the way *I* work — not building a universal tool. That said,
> customization is welcome and feedback is encouraged: if something is in your
> way or you'd like to bend it to your workflow, open an issue or a PR.

A terminal workspace for juggling multiple projects: a Vim-style editor,
embedded terminals, Claude agent sessions, a Git view, fuzzy file finder, and
project-wide grep — all in one TUI.

Written in Rust on `ratatui` + `crossterm`, with a local SQLite database
storing project state, open tabs, and feature metadata.

## Features

- **Multiple projects at once** — project tabs (Ctrl+J/K), independent state
  per project (open file, tree, terminals, agents).
- **Modal editor** — Normal / Insert / Visual / VisualLine / Search, Vim-style
  motions, syntax highlighting via `syntect`.
- **File tree + Changes** — toggle the left pane (`<Space>c`), Git status
  shown inline on file entries.
- **Fuzzy file finder** (`<Space>f`) and project **grep** (`<Space>g`).
- **Embedded terminals** (PowerShell on Windows, bash elsewhere) with a real
  PTY and scrollback.
- **Per-project Claude agent sessions** with conversation resume.
- **Git view** — branches, commits, `gh` integration for pull requests,
  and `git worktree` management (list / create / delete / open).
- **Project view** — project meta (About / Conventions / AI Hints / AI Notes)
  plus a feature list with steps and comments.
- **Runtime view** — declare services in `CoffeeTable.Runtime.yaml` and start,
  stop, restart, or build them from inside the app. Streams stdout/stderr into
  a tagged shared log (filter to one service) and shows PID + CPU / RAM /
  disk-IO per process.
- **AI commit** (`<Space>C`) — generates a commit message via Claude CLI,
  lets you review and confirm.
- **Vim-style command palette** (`Ctrl+P`) — `:w`, `:q`, `:e`, `:S`, `:H`, `:D`…

## Requirements

- **Rust** ≥ 1.85 (edition 2024) — `rustup update stable`
- **Git** on `PATH`
- (Optional) **Claude Code CLI** as `claude` on `PATH` — required for agents
  and AI commit
- (Optional) **`gh`** — for pull request operations from the Git view
- A terminal with truecolor support and a Nerd Font (for file icons)

## Installation

```bash
git clone <repo-url> coffeetable
cd coffeetable
cargo build --release
```

The binary lands in `target/release/coffeetable` (`coffeetable.exe` on
Windows). Copy it somewhere on your `PATH` or add an alias.

Or install directly:

```bash
cargo install --path .
```

## First run

```bash
coffeetable
```

On first start the app creates a user data directory and writes a default
`settings.yaml`:

| Platform | Location |
|----------|----------|
| Windows  | `%APPDATA%\coffeetable\coffeetable\data\` |
| macOS    | `~/Library/Application Support/dev.coffeetable.coffeetable/` |
| Linux    | `~/.local/share/coffeetable/` |

That directory holds:

- `settings.yaml` — configuration (editable, see below)
- `coffeetable.db` — SQLite with projects, tabs, and features
- `agents/project_<id>/` — per-project agent session context

## Configuration (`settings.yaml`)

```yaml
roots:
  - C:/Workspace/PRV
  - C:/Workspace/SL

search_excludes:
  - node_modules
  - .next
  - .git
  - .idea
  - .vscode
  - bin
  - obj

ai:
  provider: claude_cli
  binary: claude          # name on PATH or absolute path
  model: null             # e.g. "claude-opus-4-7" — null = client default
  extra_args: []

shell:
  command: powershell     # bash on Linux/macOS
  args: []
```

- **`roots`** — directories the project picker scans for repositories.
- **`search_excludes`** — folders skipped by grep and the fuzzy finder.
- **`ai.binary`** — used by agents and AI commit. Defaults to `claude`.
- **`shell`** — startup command for embedded terminals.

You can open the file from inside the app with `:S` (or `:settings`).

## Controls

### Global

| Key           | Action |
|---------------|--------|
| `Space`       | Leader — opens the quick-action menu |
| `Ctrl+P`      | Command palette (Vim-style) |
| `?`           | Help |
| `Ctrl+J / K`  | Switch active project tab |
| `q`           | Quit (from Normal mode) |

### Leader (`Space` + ...)

| Key | Action |
|-----|--------|
| `p` | Project picker |
| `f` | Find file (fuzzy) |
| `g` | Grep |
| `c` | Toggle left pane: tree ↔ changes |
| `e` | Focus the tree |
| `b` | Focus the editor |
| `w` | Show working copy (editable) |
| `h` | Show HEAD version (read-only) |
| `d` | Show diff vs HEAD |
| `C` | **AI commit** — generate a message with Claude |
| `t` | Terminal (focus existing or create the first) |
| `T` | New terminal |
| `P` | Project view (meta + features) |
| `G` | Git view (branches + commits) |
| `r` | Runtime view (services defined in `CoffeeTable.Runtime.yaml`) |
| `a` | Agent for the selected feature |
| `z` | Toggle wrap (none / 120 / 80) |

### Editor

Keys behave like Vim. Basics:

- `i` insert, `a` append, `o` / `O` new line, `Esc` back to Normal
- `v` visual, `V` visual line
- `h j k l`, `w b e`, `0 $`, `gg G`, `{ }` — motions
- `dd`, `yy`, `p`, `u`, `Ctrl+R` — editing and undo
- `/` search, `n` / `N` next / previous
- `Ctrl+S` save

Command palette (`Ctrl+P`):

| Command | Aliases       | Action |
|---------|---------------|--------|
| `:w`    | `:write`      | Save |
| `:q`    | `:close`      | Close editor (`:q!` forces) |
| `:x`    | `:wq`         | Save and close |
| `:e`    | `:reload`     | Reload from disk (`:e!` discards changes) |
| `:Q`    | `:qa`, `:quit`| Quit the application (`:Q!` forces) |
| `:f`    | `:find`       | Find file |
| `:g`    | `:grep`       | Grep |
| `:p`    | `:projects`   | Project picker |
| `:S`    | `:settings`   | Open `settings.yaml` |
| `:H`    | `:head`       | HEAD version |
| `:W`    | `:working`    | Back to working copy |
| `:D`    | `:diff`       | Diff vs HEAD |
| `:runtime` |            | Switch to the Runtime view |
| `:run [name]`  |        | Run all services (or just `name`) |
| `:stop [name]` |        | Stop all services (or just `name`) |
| `:build [name]` |       | Run the build step for all services (or `name`) |
| `:restart [name]` |     | Stop + start (all or `name`) |

### Terminal

Inside the embedded terminal all keys go to the shell. To trigger a
CoffeeTable action, use the prefix **`Ctrl+Space`** — a menu of letters pops
up (e.g. `Ctrl+Space d` = detach back to the editor, `Ctrl+Space n` = new
tab, `Ctrl+Space x` = close current tab).

### Agents

Same `Ctrl+Space` prefix as the terminal, scoped to agent sessions:

| Keys | Action |
|------|--------|
| `Ctrl+Space n` | New agent session in the active project |
| `Ctrl+Space x` | **Close** the active agent — kills the child process and forgets the saved session id |
| `Ctrl+Space r` | Rename the active agent |
| `Ctrl+Space h / l` (or `Ctrl+H / Ctrl+L`) | Switch between agent tabs |
| `Ctrl+Space d` | Detach back to the editor view |

### Git view

| Key | Action |
|-----|--------|
| `j / k`, `↓ / ↑` | Move within the focused pane |
| `Tab` / `Shift+Tab` | Cycle panes (branches → commits → details) |
| `Enter` / `l` | Drill in (branch → commits → commit details / PR view / open worktree) |
| `Esc` / `Backspace` | Back out of PR view → PR list → commit details |
| `c` | Checkout selected branch |
| `p` / `P` | Push (auto sets upstream) / pull (`--ff-only`) |
| `m` | Merge selected branch into current |
| `R` / `V` | Create PR (uses HEAD subject as title) / list PRs for the selected branch |
| `W` | Show worktrees in the details pane |
| `n` | Create a new worktree (prompts for branch name; path is `<repo>-<branch>` next to the repo) |
| `D` | Delete the selected worktree (auto-retries with `--force` if needed) |
| `Enter` on a worktree | Open it as a project tab (switches if already open) |
| `r` | Refresh branches + commits |

## Views

Each project has the following views (tabs at the top, leader shortcuts
`P`, `G`, `t`, `r`):

- **Editor** — tree / changes on the left, editor on the right.
- **Terminal** — multiple terminal tabs per project.
- **Agents** — per-project Claude agent sessions, with resume.
- **Project** — project description (About, Conventions, AI Hints, AI Notes)
  and a list of features (status, description, steps, comments).
- **Runtime** — service supervisor. Reads `CoffeeTable.Runtime.yaml` from the
  project root; see [Runtime view](#runtime-view) below.
- **Git** — branch tree, commit list, `gh` integration for PRs.

## Runtime view

Drop a `CoffeeTable.Runtime.yaml` at the root of a project to declare the
services that make it up. The Runtime view (`Space r`) then lets you start,
stop, restart, or build those services and streams their output into a
shared, tagged log.

```yaml
services:
  - name: api
    command: dotnet run --project src/Api
    build: dotnet build src/Api
    env:
      ASPNETCORE_ENVIRONMENT: Development

  - name: web
    command: npm run dev
    cwd: web
    depends_on: [api]
    build: npm run build
```

Per service:

- `name` — unique identifier shown in the list and used to tag log lines.
- `command` — the foreground process to run (whitespace-split argv;
  use quotes for arguments with spaces).
- `cwd` — optional, relative to the project root.
- `build` — optional one-shot build command; runs and waits to completion.
- `depends_on` — services that must start before this one.
- `env` — extra environment variables.

### Controls (inside the Runtime view)

| Key | Action |
|-----|--------|
| `j / k`, `↓ / ↑` | Move selection |
| `g / G`          | First / last service |
| `Enter` or `f`   | Toggle output filter to the selected service |
| `Esc`            | Clear output filter |
| `c`              | Clear the output buffer |
| `e`              | Reload `CoffeeTable.Runtime.yaml` |
| `r` / `R`        | Run selected service / run all (dependencies first) |
| `s` / `S`        | Stop selected / stop all |
| `b` / `B`        | Build selected / build all |
| `x` / `X`        | Restart (stop + start) selected / all |

The same actions are available from the command palette
(`:run`, `:stop`, `:build`, `:restart`) with an optional service-name argument.

For each running process the list shows the PID along with current CPU %,
RAM, and cumulative disk-IO (sampled via `sysinfo`). GPU usage is not yet
reported.

## AI / Claude

Agents and AI commit shell out to the binary configured in `settings.yaml`
(`ai.binary`, defaults to `claude`). If you don't have
[Claude Code](https://claude.com/claude-code) installed, the AI features just
won't start — the rest of the app works without them.

## License

Source-available under [FSL-1.1-Apache-2.0](LICENSE.md): free for personal and
internal business use, commercial competing use prohibited. Each release
converts to Apache 2.0 two years after publication.
