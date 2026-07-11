<div align="center">
  <img alt="obsidian-mcp-rs logo" src="https://raw.githubusercontent.com/MrRefactoring/obsidian-mcp-rs/master/assets/logo.svg" width="120"/>

  <h1>obsidian-mcp-rs</h1>

  <a href="https://claude.ai" target="_blank" rel="noopener noreferrer"><img alt="Claude Ready" src="https://img.shields.io/badge/Claude-Ready-CC785C?style=flat-square&logo=anthropic&logoColor=white"/></a>
  <a href="https://cursor.com" target="_blank" rel="noopener noreferrer"><img alt="Cursor Ready" src="https://img.shields.io/badge/Cursor-Ready-000000?style=flat-square&logoColor=white"/></a>
  <img alt="MCP Native" src="https://img.shields.io/badge/MCP-Native-6366f1?style=flat-square"/>
  <img alt="Rust Powered" src="https://img.shields.io/badge/Rust-Powered-CE412B?style=flat-square&logo=rust&logoColor=white"/>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npx Compatible" src="https://img.shields.io/badge/npx-Compatible-CB3837?style=flat-square&logo=npm&logoColor=white"/></a>

  <br/>
  <br/>

  <a href="https://github.com/MrRefactoring/obsidian-mcp-rs/actions/workflows/ci.yml" target="_blank" rel="noopener noreferrer"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/MrRefactoring/obsidian-mcp-rs/.github/workflows/ci.yml?branch=master&style=flat-square"/></a>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npm version" src="https://img.shields.io/npm/v/obsidian-mcp-rs.svg?style=flat-square"/></a>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npm downloads" src="https://img.shields.io/npm/dm/obsidian-mcp-rs.svg?style=flat-square"/></a>
  <a href="LICENSE" target="_blank" rel="noopener noreferrer"><img alt="License: MIT" src="https://img.shields.io/github/license/MrRefactoring/obsidian-mcp-rs?color=green&style=flat-square"/></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue?style=flat-square"/>
  <a href="https://codecov.io/gh/MrRefactoring/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="Coverage" src="https://img.shields.io/codecov/c/github/mrrefactoring/obsidian-mcp-rs?style=flat-square"/></a>

  <br/>
  <br/>

  <span>MCP-сервер на Rust, подключающий ваш Obsidian vault к Claude, Cursor и любому AI-клиенту — один бинарник, никаких зависимостей в рантайме.</span>
</div>

<div align="center">

[English](README.md) | **Русский**

</div>

<br/>

> [!WARNING]
> Этот MCP-сервер имеет **полный доступ на чтение и запись** к вашему Obsidian vault. Он может создавать, редактировать, перемещать и удалять заметки без подтверждения. Используйте на свой страх и риск. Перед подключением к AI-клиенту всегда делайте резервные копии vault.
>
> Чтобы ограничить сервер режимом только для чтения, используйте `--no-edit` — см. [Режим только для чтения](#режим-только-для-чтения---no-edit).

## Установка

**Самый быстрый способ — просто попросите вашего AI-агента установить сервер.** Если вы уже работаете внутри агентного клиента (Claude Code, Cursor, Windsurf, …), вам вообще не нужно трогать конфиг — вставьте один промпт, и агент сам запустит инсталлер. Подставьте свой путь к vault:

> Установи MCP-сервер **obsidian-mcp-rs** для этого редактора. Мой Obsidian vault находится в `~/Documents/Obsidian/MyVault`. Запусти подходящий инсталлер, например `npx -y obsidian-mcp-rs install claude-code ~/Documents/Obsidian/MyVault` (для других клиентов используй `cursor`, `windsurf`, `vscode`, `claude`, …), затем напомни мне перезапустить сессию и подтвердить сервер, если клиент попросит.

У **Claude Code** есть нативный MCP CLI, поэтому его можно попросить выполнить:

```bash
claude mcp add obsidian -- npx -y obsidian-mcp-rs ~/Documents/Obsidian/MyVault
# добавьте `--scope user`, чтобы включить сервер во всех проектах (пишет в ~/.claude.json)
```

> **Важно:** клиенты читают MCP-конфиг **при старте сессии**, поэтому агент может его записать, но не подхватит на лету. После установки **перезапустите** клиент — а в Claude Code подтвердите project-scoped сервер из `.mcp.json` через панель `/mcp` — и только тогда появятся 11 инструментов. Нативный `mcp add` есть только у Claude Code; для остальных клиентов агент просто выполняет команду `npx obsidian-mcp-rs install <client>` выше.

### Предпочитаете CLI? (или не используете агента)

Не внутри агентного клиента — например, **Claude Desktop**, который не умеет выполнять shell-команды, — или хотите всё сделать сами? Интерактивный мастер сканирует установленные AI-клиенты, позволяет выбрать место установки и автоматически записывает конфигурацию:

```bash
npx obsidian-mcp-rs install
```

Или установите напрямую без интерактивного режима:

```bash
# Claude Desktop
npx obsidian-mcp-rs install claude ~/Documents/Obsidian/MyVault

# Claude Code — локально для проекта (.mcp.json в текущей папке)
npx obsidian-mcp-rs install claude-code ~/vault

# Claude Code — глобально (~/.claude.json)
npx obsidian-mcp-rs install claude-code --global ~/vault

# Cursor — локально для проекта (.cursor/mcp.json в текущей папке)
npx obsidian-mcp-rs install cursor ~/vault

# Cursor — глобально (~/.cursor/mcp.json)
npx obsidian-mcp-rs install cursor --global ~/vault

# OpenClaw
npx obsidian-mcp-rs install openclaw ~/vault

# Несколько vault
npx obsidian-mcp-rs install claude ~/vault1 ~/vault2
```

Другие команды управления:

```bash
npx obsidian-mcp-rs list       # показать статус установки по всем клиентам
npx obsidian-mcp-rs uninstall  # интерактивный мастер удаления
npx obsidian-mcp-rs uninstall claude --dry-run  # предварительный просмотр без записи
```

## Возможности

- **11 инструментов** для создания, чтения, обновления, удаления заметок, поиска, управления папками и тегами
- **Несколько vault** — передайте несколько путей в аргументах
- **Режим только для чтения** — флаг `--no-edit` отключает все инструменты записи на уровне сервера
- **Никаких зависимостей в рантайме** — один статический бинарник, Node.js для запуска не требуется
- **Кросс-платформенность** — macOS (ARM64 + x64), Linux (x64 + ARM64 + musl), Windows (x64 + ARM64)
- **Поиск по тегам** через префикс `tag:` в запросах
- **YAML frontmatter** — управление тегами в метаданных заметок
- **Совместимость с `npx`** — запускается мгновенно через npm

## Производительность

Операции по всему vault (`search-vault`, `rename-tag`) обходят vault с помощью крейта [`ignore`](https://crates.io/crates/ignore) и обрабатывают файлы параллельно через [`rayon`](https://crates.io/crates/rayon). Замерено набором criterion в [`benches/`](benches/vault_bench.rs) на синтетическом vault, Apple Silicon (10 логических ядер); «последовательно» — тот же код, ограниченный одним потоком (`RAYON_NUM_THREADS=1`):

| Операция                       | Последовательно (1 поток) | Параллельно | Ускорение |
| ------------------------------ | ------------------------- | ----------- | --------- |
| Поиск по содержимому (2000)    | 52.8 ms                   | 26.3 ms     | ~2.0×     |
| Поиск по тегам (2000)          | 45.6 ms                   | 24.4 ms     | ~1.9×     |
| Переименование тега (500)      | 84.3 ms                   | 60.0 ms     | ~1.4×     |

Операции с одной заметкой (`read-note`, `create-note`, `edit-note`, …) затрагивают один файл и не изменились. Числа зависят от количества ядер и диска; воспроизвести локально можно через `cargo bench`.

## Установка

```bash
npm install -g obsidian-mcp-rs
```

Или используйте напрямую без установки (рекомендуется):

```bash
npx obsidian-mcp-rs install   # мастер сам запишет конфигурацию
```

## Настройка

> **Совет:** `npx obsidian-mcp-rs install` записывает эти конфигурации автоматически. Разделы ниже — для ручной настройки или справки.

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"]
    }
  }
}
```

### Несколько vault

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": [
        "-y",
        "obsidian-mcp-rs",
        "/path/to/vault1",
        "/path/to/vault2"
      ]
    }
  }
}
```

### Claude Code (`.mcp.json` / `~/.claude.json`)

Конфиг Claude Code содержит явное `"type": "stdio"` (Claude Desktop выше — без него):

```json
{
  "mcpServers": {
    "obsidian": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "~/Documents/Obsidian/MyVault"]
    }
  }
}
```

### Cursor

Добавьте сервер через **Settings → MCP → Add Server** или отредактируйте `~/.cursor/mcp.json` напрямую:

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"]
    }
  }
}
```

После добавления AI в Cursor получит доступ ко всем 11 инструментам vault. Проверить можно в панели MCP в Settings.

### OpenClaw (`~/.openclaw/openclaw.json`)

```json
{
  "mcp": {
    "servers": {
      "obsidian": {
        "command": "npx",
        "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"],
        "transport": "stdio"
      }
    }
  }
}
```

## Режим только для чтения (`--no-edit`)

Передайте `--no-edit`, чтобы запустить сервер в режиме только для чтения. Все инструменты записи немедленно возвращают ошибку — файлы vault не изменяются.

**Инструменты только для чтения (всегда доступны):**
- `read-note`, `search-vault`, `list-available-vaults`

**Заблокированные инструменты при `--no-edit`:**
- `create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `add-tags`, `remove-tags`, `rename-tag`

### Ручная настройка с `--no-edit`

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "--no-edit", "/path/to/your/vault"]
    }
  }
}
```

### Через мастер `install`

```bash
npx obsidian-mcp-rs install claude --no-edit ~/Documents/Obsidian/MyVault
```

## Поддерживаемые платформы

| Платформа | Архитектура | Target triple |
|-----------|-------------|---------------|
| macOS | ARM64 (Apple Silicon) | `aarch64-apple-darwin` |
| macOS | x64 (Intel) | `x86_64-apple-darwin` |
| Linux | x64 (glibc) | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 (glibc) | `aarch64-unknown-linux-gnu` |
| Linux | x64 (musl / Alpine) | `x86_64-unknown-linux-musl` |
| Windows | x64 | `x86_64-pc-windows-msvc` |
| Windows | ARM64 | `aarch64-pc-windows-msvc` |

## Справочник инструментов

### `read-note`
Читает содержимое существующей заметки.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Имя файла заметки (`.md` необязательно) |
| `folder` | string | | Путь к подпапке внутри vault |

### `create-note`
Создаёт новую заметку с Markdown-содержимым.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Имя файла заметки |
| `content` | string | ✓ | Markdown-содержимое |
| `folder` | string | | Путь к подпапке (создаётся автоматически) |

### `edit-note`
Редактирует существующую заметку.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Имя файла заметки |
| `operation` | string | ✓ | `append`, `prepend`, `replace`, `find_and_replace` |
| `content` | string | ✓ | Применяемое содержимое |
| `folder` | string | | Путь к подпапке |
| `search` | string | | Искомый текст (обязателен для `find_and_replace`) |

### `delete-note`
Удаляет заметку из vault.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Имя файла заметки |
| `folder` | string | | Путь к подпапке |

### `move-note`
Перемещает или переименовывает заметку внутри vault.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Исходное имя файла |
| `folder` | string | | Исходная папка |
| `newFolder` | string | | Папка назначения |
| `newFilename` | string | | Новое имя файла (не изменяется, если не указано) |

### `create-directory`
Создаёт новую папку в vault.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `path` | string | ✓ | Путь к папке относительно корня vault |
| `recursive` | boolean | | Создавать родительские папки (по умолчанию: `true`) |

### `search-vault`
Ищет заметки по содержимому, имени файла или тегу.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `query` | string | ✓ | Поисковый запрос. Используйте `tag:имя` для поиска по тегам |
| `path` | string | | Ограничить поиск подпапкой |
| `caseSensitive` | boolean | | По умолчанию: `false` |
| `searchType` | string | | `content` (по умолчанию), `filename`, `both` |

### `add-tags`
Добавляет теги в заметки через frontmatter и/или содержимое.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `files` | string[] | ✓ | Имена файлов заметок (с `.md`) |
| `tags` | string[] | ✓ | Теги для добавления |
| `location` | string | | `frontmatter`, `content`, `both` (по умолчанию) |
| `normalize` | boolean | | Нормализовать формат тегов (по умолчанию: `true`) |
| `position` | string | | `start` или `end` (по умолчанию) для тегов в содержимом |

### `remove-tags`
Удаляет теги из заметок.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `files` | string[] | ✓ | Имена файлов заметок |
| `tags` | string[] | ✓ | Теги для удаления |

### `rename-tag`
Переименовывает тег во всех заметках vault.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `oldTag` | string | ✓ | Текущее имя тега |
| `newTag` | string | ✓ | Новое имя тега |

### `list-available-vaults`
Выводит список всех vault, настроенных для этого сервера. Параметров нет.

## Разработка

### Требования

- [Rust](https://rustup.rs/) (stable, 1.94+)
- [Node.js](https://nodejs.org/) 22+ (для npm-обёртки)

### Сборка из исходников

```bash
git clone https://github.com/MrRefactoring/obsidian-mcp-rs.git
cd obsidian-mcp-rs

# Собрать Rust-бинарник
cargo build --release

# Собрать TypeScript-обёртку
cd npm/obsidian-mcp-rs
npm install
npm run build

# Запустить напрямую
./target/release/obsidian-mcp-rs /path/to/your/vault
```

### Тестирование

```bash
cargo test               # все тесты (lib + интеграционные)
cargo test --lib         # только модульные тесты библиотеки
```

### Бенчмарки

```bash
cargo bench                          # запустить набор criterion в benches/
RAYON_NUM_THREADS=1 cargo bench      # однопоточный baseline для сравнения
cargo bench --no-run                 # только компиляция (то, что гоняет CI)
```

### Кросс-компиляция

Для Linux требуется [cross](https://github.com/cross-rs/cross):

```bash
cargo install cross --git https://github.com/cross-rs/cross

cross build --release --target aarch64-unknown-linux-gnu
cross build --release --target x86_64-unknown-linux-musl
```

### Переменные окружения

| Переменная | Описание |
|------------|----------|
| `RUST_LOG` | Уровень логирования: `error`, `warn` (по умолчанию), `info`, `debug`, `trace` |

Логи пишутся в **stderr** — stdout зарезервирован для MCP JSON-RPC.

## Диагностика

При работе сервера в фоновом режиме stderr перехватывается MCP-клиентом и может быть недоступен. Поэтому obsidian-mcp-rs **автоматически пишет DEBUG-логи в файл** при каждом запуске.

### Расположение лог-файла

| Платформа | Путь по умолчанию |
|-----------|-------------------|
| macOS | `~/Library/Logs/obsidian-mcp-rs/obsidian-mcp-rs.log` |
| Linux | `~/.local/share/obsidian-mcp-rs/obsidian-mcp-rs.log` |
| Windows | `%LOCALAPPDATA%\obsidian-mcp-rs\obsidian-mcp-rs.log` |

### Просмотр логов и ссылка на баг-репорт

```bash
npx obsidian-mcp-rs logs
```

Выводит путь к лог-файлу, последние 100 строк и ссылку для открытия GitHub-issue.

### Подробный вывод в stderr

Удобно при ручном запуске сервера в терминале:

```bash
obsidian-mcp-rs --verbose /path/to/vault
# эквивалентно:
RUST_LOG=debug obsidian-mcp-rs /path/to/vault
```

### Пользовательский лог-файл

```bash
# Записать по конкретному пути:
obsidian-mcp-rs --log-file /tmp/mcp-debug.log /path/to/vault

# Полностью отключить запись в файл:
obsidian-mcp-rs --log-file - /path/to/vault
```

### Как сообщить об ошибке

1. Выполните `npx obsidian-mcp-rs logs`
2. Скопируйте вывод (или прикрепите лог-файл)
3. Откройте issue: <https://github.com/MrRefactoring/obsidian-mcp-rs/issues/new>

## Архитектура

```
npx obsidian-mcp-rs /vault/path
          │
          ▼
  npm/obsidian-mcp-rs/bin/bin.js   ← TypeScript: определение платформы
          │   определяет ОС + архитектуру
          │   подключает @obsidian-mcp-rs/<platform>
          ▼
  obsidian-mcp-rs (Rust binary)   ← MCP-сервер, stdio transport
          │
          ├── clap → разбор аргументов CLI
          ├── VaultManager → операции с файловой системой
          ├── ObsidianHandler → 11 реализаций MCP-инструментов
          └── rmcp → JSON-RPC / MCP-протокол
```

## Участие в разработке

1. Сделайте форк репозитория
2. Создайте ветку для фичи: `git checkout -b feat/my-feature`
3. Реализуйте с тестами
4. Убедитесь, что `cargo fmt` и `cargo clippy` проходят без ошибок
5. Отправьте pull request

## Лицензия

MIT — см. [LICENSE](LICENSE).
