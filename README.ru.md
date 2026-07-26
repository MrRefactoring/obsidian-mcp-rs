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

> **Node.js 22 или новее нужен, чтобы *запустить инсталлер*** — именно так он распространяется.
> Для **работы сервера** Node не нужен: инсталлер кладёт один статический бинарник и прописывает клиенту путь прямо к нему, так что дальше не работает ничего, кроме этого бинарника. Если Node не нужен вовсе — скачайте бинарник со страницы [Releases](https://github.com/MrRefactoring/obsidian-mcp-rs/releases) или выполните `cargo install obsidian-mcp-rs`, а затем запустите его подкоманду `install`.

**Самый быстрый способ — просто попросите вашего AI-агента установить сервер.** Если вы уже работаете внутри агентного клиента (Claude Code, Cursor, Windsurf, …), вам вообще не нужно трогать конфиг — вставьте один промпт, и агент сам запустит инсталлер. Подставьте свой путь к vault:

> Установи MCP-сервер **obsidian-mcp-rs** для этого редактора. Мой Obsidian vault находится в `~/Documents/Obsidian/MyVault`. Выполни `npx -y obsidian-mcp-rs install claude-code ~/Documents/Obsidian/MyVault` (для других клиентов — `cursor`, `windsurf`, `vscode`, `claude`, …). Инсталлер копирует сервер в постоянное место и прописывает этот путь в мой конфиг — скажи, куда он его положил, и напомни, что обновление это повторный запуск той же команды, а не `npm update`. Затем напомни перезапустить сессию и подтвердить сервер, если клиент попросит.

> **Важно:** клиенты читают MCP-конфиг **при старте сессии**, поэтому агент может его записать, но не подхватит на лету. После установки **перезапустите** клиент — а в Claude Code подтвердите project-scoped сервер из `.mcp.json` через панель `/mcp` — и только тогда появятся 15 инструментов.

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
npx obsidian-mcp-rs list       # статус установки по всем клиентам и версия установленного сервера
npx obsidian-mcp-rs uninstall  # интерактивный мастер удаления
npx obsidian-mcp-rs uninstall claude --dry-run  # предварительный просмотр без записи
```

### Что именно записывает `install`

Он копирует бинарник сервера в постоянное место для текущего пользователя и записывает в конфиг клиента **этот абсолютный путь**:

| Платформа | Установленный сервер |
|-----------|----------------------|
| macOS     | `~/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs` |
| Linux     | `~/.local/share/obsidian-mcp-rs/bin/obsidian-mcp-rs` |
| Windows   | `%LOCALAPPDATA%\obsidian-mcp-rs\bin\obsidian-mcp-rs.exe` |

Поэтому ваш конфиг запускает **один процесс** — сам сервер, прямым потомком AI-клиента. Он не запускает `npx`, который поднял бы три (npm → Node-лаунчер → сервер) и оставил бы два лишних всякий раз, когда клиент завершает только первый. Кроме того, сервер благодаря этому видит, что клиент закрылся, и уходит вместе с ним, а не остаётся висеть с правом записи в ваш vault.

`npx` по-прежнему остаётся способом *запустить инсталлер* и самым быстрым способом попробовать сервер. Он просто больше не то, что навсегда попадает в конфиг.

### Обновление

```bash
npx obsidian-mcp-rs@latest install    # та же команда, что и при первой установке
```

Она заменяет установленный бинарник на месте. **Путь в конфигах не меняется никогда**, поэтому ничего не нужно перенастраивать и ни один конфиг не протухает. (Одно исключение, и оно касается всех, кто ставился рано: в конфиге, созданном до 0.7.0, этого пути ещё нет, и сам по себе `install` его не перезапишет — см. [ниже](#обновление-конфига-созданного-до-070).)

Два момента, о которых стоит знать:

- **Один `npm update` не обновляет установленный сервер.** Копия, которую запускает клиент, меняется только при запуске `install`. Команда `npx obsidian-mcp-rs list` показывает версию установленного сервера рядом с версией пакета и предупреждает, когда они разошлись.
- **На Windows сначала закройте AI-клиенты.** Windows не даёт перезаписать запущенный исполняемый файл; если клиент всё ещё держит сервер открытым, инсталлер скажет об этом и попросит закрыть клиент.

`uninstall` удаляет и сам бинарник — как только на него не ссылается ни один конфиг.

### Обновление конфига, созданного до 0.7.0

Всё описанное выше относится к конфигу, который написал вам `install`. Если ваш появился до 0.7.0, в нём лежит `npx -y obsidian-mcp-rs` или разрешённый путь внутрь npm-кеша `_npx`, и **повторный запуск `install` его не заменит.**

Инсталлер не перезаписывает запись, которая отличается от той, что он собирается написать: эту запись могли править руками, и молча её выбросить хуже, чем не сделать ничего. Поэтому он сообщает о ней и идёт дальше:

```
! Claude Code – global (~/.claude.json)  already installed in ~/.claude.json, but with different settings than you asked for
    nothing was changed — re-run with --force to replace that entry
```

Эта строка стоит между клиентами, которые *были* записаны, и финальным «перезапустите клиент», — её легко пропустить. Если после обновления клиент всё ещё стартует `npx`, причина в этом, и ни одно из исправлений жизненного цикла выше для него не действует.

Чтобы мигрировать, добавьте `--force`:

```bash
npx obsidian-mcp-rs@latest install --force
```

Каждый бэкенд перед записью копирует прежний файл в `<config>.bak`, так что откат есть.

`list` называет те, которым это нужно:

```
! Claude Desktop                          outdated   …/claude_desktop_config.json — runs `obsidian-vault-mcp`, not the installed server; re-run `install --force`
! Claude Code – global (~/.claude.json)   outdated   ~/.claude.json — runs `npx`, not the installed server; re-run `install --force`
✓ Codex CLI – global                      installed  ~/.codex/config.toml
```

`outdated` значит, что запись есть и запускает не этот сервер. В прежних версиях все они показывались как `installed`, потому что проверка спрашивала только, существует ли ключ `obsidian`, — по той же причине незамеченной оставалась и запись, ведущая на удалённый бинарник.

### Известные проблемы, которые не на нашей стороне

- **Дублирование процессов в Claude Desktop.** Claude Desktop может поднять больше одной копии одного и того же MCP-сервера за запуск ([claude-code#36616](https://github.com/anthropics/claude-code/issues/36616)). Этот сервер не является причиной и не может это предотвратить. Это безопасно: одновременные серверы на одном vault сериализуются и не теряют правки друг друга, а те, что пережили своего клиента, выходят сами.
- **Осиротевшие MCP-процессы вообще.** Некоторые клиенты не завершают stdio MCP-серверы при нештатном выходе ([#22612](https://github.com/anthropics/claude-code/issues/22612), [#1935](https://github.com/anthropics/claude-code/issues/1935), [#40667](https://github.com/anthropics/claude-code/issues/40667)). Этот сервер следит за процессом, который его запустил, и выходит вместе с ним — на macOS и Linux. На Windows такой подстраховки пока нет.
- **Два устройства и один синхронизируемый vault.** Записи сериализуются в пределах машины. Два компьютера, одновременно правящие один облачный vault, — это конфликт синхронизации, и он относится к iCloud / Obsidian Sync, а не к серверу.

## Возможности

- **15 инструментов** — CRUD заметок, поиск, ссылки, frontmatter, ежедневные заметки, управление папками и операции с тегами
- **Ранжированный поиск** — релевантность по BM25 с усилением полей (слово в заголовке весит больше, чем то же слово где-то в середине абзаца): результаты отдаются от лучших к худшим и ограничиваются по количеству, чтобы частотное слово не завалило контекст модели
- **Перемещение с учётом ссылок** — при переименовании заметки переписываются все указывающие на неё `[[wikilink]]` и Markdown-ссылки, так что перемещение никогда не оставляет ссылки висеть в пустоте
- **Граф ссылок** — `wikilinks` отвечает на вопросы про backlinks, исходящие ссылки, битые ссылки и заметки-сироты
- **Правки на уровне секции** — наведите `edit-note` на конкретный заголовок или на `^block-id`, и переписаны будут только эти байты; остальная часть заметки проходит насквозь нетронутой
- **Доступ к frontmatter** — `frontmatter` читает и пишет любой YAML-ключ, а не только `tags`, и затрагивает лишь тот ключ, который вы назвали
- **Несколько vault** — передайте несколько путей в аргументах
- **Обратимое удаление** — `delete-note` переносит заметку в `.trash/` внутри vault (как это делает сам Obsidian), а не стирает её: заметка исчезает из поиска и из графа ссылок, но пользователь всегда может вернуть её обратно
- **Ежедневные заметки** — `periodic` читает и создаёт заметки от daily до yearly, опираясь на *собственные* настройки Obsidian (формат имени, папка, шаблон), поэтому пишет ровно в ту заметку, которой вы реально пользуетесь
- **Обзор vault** — `vault-info` отвечает, какие теги существуют, что менялось недавно и насколько велик vault
- **Режим только для чтения** — `--no-edit` полностью убирает инструменты записи из `tools/list`, так что сервер, доступный только на чтение, именно так себя и описывает
- **Никаких зависимостей в рантайме** — сервер представляет собой один статический бинарник. (Node.js 22+ нужен только для установки через `npx`; скачайте бинарник из [Releases](https://github.com/MrRefactoring/obsidian-mcp-rs/releases) или выполните `cargo install`, чтобы обойтись без него.)
- **Кросс-платформенность** — macOS (ARM64 + x64), Linux (x64 + ARM64 + musl), Windows (x64 + ARM64)
- **Поиск по тегам** через префикс `tag:` в запросах
- **YAML frontmatter** — управление тегами в метаданных заметок
- **Streamable HTTP** (опционально) — `cargo install obsidian-mcp-rs --features http`, после чего флаг `--http` позволяет обслуживать несколько клиентов из одного долгоживущего сервера. Заголовок `Origin` проверяется, как того требует спецификация MCP от локальных серверов. По умолчанию по-прежнему используется stdio.
- **Совместимость с `npx`** — запускается мгновенно через npm

### Поиск

`search-vault` ранжирует совпадения по **BM25** — тому же семейству метрик, что и полнотекстовые движки, — но считает их прямо во время параллельного обхода vault. Поэтому индекс строить не нужно, watcher синхронизировать не с чем, и ничего не устаревает, когда вы правите заметку в самом Obsidian.

Термины взвешиваются по месту вхождения: имя файла ×5, теги ×4, заголовки ×3, frontmatter ×2, текст ×1. Редкие слова весят больше частотных, поэтому по запросу вроде `the kafka` заметка *про* Kafka окажется выше заметки, где просто много раз встречается «the».

Результаты отдаются страницами (`limit`, по умолчанию 20; `offset`), и из каждого файла цитируется не больше `maxMatchesPerFile` строк (по умолчанию 3). В каждом ответе есть `total` и `truncated`, так что модель видит, что совпадений больше, — но не платит за них контекстом.

Ранжирование отвечает на вопрос «какие заметки *про* это». На два вопроса, которые ему не по силам, есть отдельные аргументы:

- **`regex: true`** — искать *форму*, а не слова: номер телефона, `TODO(name)`, URL. Совпадения ранжируются по количеству подошедших строк, потому что для шаблона релевантность не имеет смысла.
- **`frontmatter: {"status": "active"}`** — оставить только заметки с указанными полями. Поле-список совпадает, если оно *содержит* значение: `{"tags": "work"}` найдёт заметку с `tags: [work, urgent]`. Это можно комбинировать с запросом, а можно использовать отдельно, с пустым запросом, как чистую выборку по метаданным («все активные заметки в этом vault»).

И то и другое вычисляется внутри обхода, который и так читает каждую заметку, поэтому лишнего прохода не требуется.

## Производительность

Операции по всему vault (`search-vault`, `rename-tag`) обходят vault с помощью крейта [`ignore`](https://crates.io/crates/ignore) и обрабатывают файлы параллельно через [`rayon`](https://crates.io/crates/rayon). Замерено набором criterion в [`benches/`](benches/vault_bench.rs) на синтетическом vault, Apple Silicon (10 логических ядер); «последовательно» — тот же код, ограниченный одним потоком (`RAYON_NUM_THREADS=1`):

| Операция                            | Последовательно (1 поток) | Параллельно | Ускорение |
| ----------------------------------- | ------------------------- | ----------- | --------- |
| Ранжированный поиск (2000 заметок)  | 52.8 ms                   | 26.2 ms     | ~2.0×     |
| Поиск по тегам (2000 заметок)       | 45.6 ms                   | 24.4 ms     | ~1.9×     |
| Переименование тега (500 заметок)   | 84.3 ms                   | 60.0 ms     | ~1.4×     |

Операции с одной заметкой (`read-note`, `create-note`, `edit-note`, …) затрагивают один файл и не изменились. Числа зависят от количества ядер и диска; воспроизвести локально можно через `cargo bench`.

## Настройка

> **Совет:** `npx obsidian-mcp-rs install` записывает эти конфигурации автоматически, включая абсолютный путь ниже — вычислять его вручную не нужно. Разделы ниже — для ручной настройки или справки.
>
> В `command` идёт путь, который сообщает `install`, например `~/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs` на macOS (полностью, без `~` — конфиги его не раскрывают). **Не** пишите сюда `npx`: он поднимает три процесса вместо одного, оставляет два из них после того, как клиент завершил сервер, и скрывает от сервера факт выхода клиента, из-за чего тот не может убрать за собой.

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["/path/to/your/vault"]
    }
  }
}
```

### Несколько vault

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["/path/to/vault1",
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
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["~/Documents/Obsidian/MyVault"]
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
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["/path/to/your/vault"]
    }
  }
}
```

После добавления AI в Cursor получит доступ ко всем 15 инструментам vault. Проверить можно в панели MCP в Settings.

### OpenClaw (`~/.openclaw/openclaw.json`)

```json
{
  "mcp": {
    "servers": {
      "obsidian": {
        "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
        "args": ["/path/to/your/vault"],
        "transport": "stdio"
      }
    }
  }
}
```

## Режим только для чтения (`--no-edit`)

Передайте `--no-edit`, чтобы запустить сервер в режиме только для чтения. Восемь инструментов, которые умеют только писать, **полностью убираются из `tools/list`** — сервер, доступный лишь на чтение, именно так себя и описывает, а не рекламирует инструменты, на которые всё равно ответит отказом, — и через `tools/call` они тоже недоступны.

**Убраны при `--no-edit`** (инструменты только записи):
`create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `add-tags`, `remove-tags`, `rename-tag`

**Остаются в списке, потому что не только пишут, но и читают** — они ограничены *по действиям*: чтение работает, запись отклоняется:
- `frontmatter` — `get` работает; `set` и `remove` отклоняются
- `periodic` — `get` и `list` работают; `create` отклоняется

**Чистое чтение, доступны всегда:**
`read-note`, `search-vault`, `wikilinks`, `vault-info`, `list-available-vaults`

### Ручная настройка с `--no-edit`

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["--no-edit", "/path/to/your/vault"]
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
| `view` | string | | `content` (по умолчанию) или `outline` — заголовки, block-ссылки и ключи frontmatter |
| `offset` | number | | Первая возвращаемая строка, нумерация с 1 (по умолчанию 1) |
| `limit` | number | | Максимум возвращаемых строк (по умолчанию 400) |

Чтение ограничено по объёму, чтобы одна длинная заметка не съела весь контекст модели. За пределом заметка обрезается маркером, который сообщает, какие строки вы получили и какой `offset` передать за остатком; заметка, укладывающаяся в лимит, приходит целиком. `offset` считает строки так же, как их печатает `view: "outline"`, поэтому номер из одного можно подставить прямо в другой.

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
| `newFolder` | string | | Папка назначения. **Не указывайте её, чтобы оставить заметку на месте** — так и переименовывают, не перемещая. Передайте `""`, чтобы перенести заметку в корень vault. |
| `newFilename` | string | | Новое имя файла (не изменяется, если не указано) |

Нужно передать хотя бы один из `newFolder` / `newFilename` — перемещение без обоих отклоняется, а не додумывается. Входящие `[[wikilinks]]` переписываются, поэтому они следуют за заметкой.

### `create-directory`
Создаёт новую папку в vault.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `path` | string | ✓ | Путь к папке относительно корня vault |
| `recursive` | boolean | | Создавать родительские папки (по умолчанию: `true`) |

### `search-vault`
Ищет заметки по содержимому, имени файла или тегу. Результаты ранжируются по BM25, отдаются от лучших к худшим и разбиваются на страницы.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `query` | string | ✓ | Поисковый запрос. `tag:имя` ищет по тегу. Может быть пустым, если фильтрация идёт только по `frontmatter` |
| `path` | string | | Ограничить поиск подпапкой |
| `caseSensitive` | boolean | | По умолчанию: `false` |
| `searchType` | string | | `content` (по умолчанию), `filename`, `both` |
| `regex` | boolean | | Читать `query` как регулярное выражение (по умолчанию `false`) |
| `frontmatter` | object | | Только заметки с указанными полями, например `{"status": "active"}`. Поле-список совпадает, если *содержит* значение |
| `limit` | number | | Сколько файлов вернуть (по умолчанию 20) |
| `offset` | number | | Пропустить столько файлов (по умолчанию 0) |
| `maxMatchesPerFile` | number | | Сколько строк цитировать из файла (по умолчанию 3) |

В каждом совпадении есть `path` — его можно сразу передать как `filename` в любой инструмент для работы с заметкой.

### `wikilinks`
Граф ссылок vault, за один параллельный проход.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `query` | string | ✓ | `backlinks`, `outgoing`, `broken` или `orphans` |
| `filename` | string | | Заметка, о которой спрашиваем, — обязательна для `backlinks` и `outgoing` |
| `folder` | string | | Подпапка, в которой лежит заметка |
| `limit` | number | | По умолчанию 50 — `broken` и `orphans` в запущенном vault легко уходят в тысячи |
| `offset` | number | | Пропустить столько результатов (по умолчанию 0) |

Ссылки внутри блоков кода игнорируются: `[[link]]` в примере кода — это документация, а не ссылка.

### `frontmatter`
Читает и пишет любой ключ YAML-frontmatter — не только `tags`. Запись выполняется построчной правкой одного названного ключа, поэтому остальная часть блока (комментарии, порядок ключей, кавычки) сохраняется байт в байт.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `filename` | string | ✓ | Путь к заметке |
| `action` | string | ✓ | `get`, `set` или `remove` |
| `key` | string | | Какое поле. Не указывайте его вместе с `get`, чтобы получить блок целиком |
| `value` | any | | Что записать — строка, число, булево значение, список или объект (только для `set`) |
| `folder` | string | | Подпапка, в которой лежит заметка |

При `--no-edit` инструмент ограничен по действиям: `get` работает, `set` и `remove` отклоняются.

### `vault-info`
Что вообще лежит в этом vault — вопросы, которые задают *до* того, как понимают, что искать.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `query` | string | ✓ | `tags` (все теги и число заметок с каждым, самые частые сверху), `recent` (сначала свежие) или `stats` |
| `limit` | number | | Ограничить длину списка (по умолчанию 20) |

### `periodic`
Сегодняшняя ежедневная заметка и её weekly/monthly/quarterly/yearly родственники — берутся из *собственных* настроек Obsidian (сначала `data.json` плагина Periodic Notes, затем встроенный `daily-notes.json`, затем значения Obsidian по умолчанию), поэтому заметка оказывается там, где её создал бы сам Obsidian, а не где-то сбоку.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `period` | string | ✓ | `daily`, `weekly`, `monthly`, `quarterly`, `yearly` |
| `action` | string | ✓ | `get`, `create` (идемпотентно) или `list` |
| `date` | string | | `YYYY-MM-DD` — по умолчанию сегодня |
| `content` | string | | Текст для заметки, которую создаёт `create`; без него используется настроенный шаблон |
| `limit` | number | | Сколько заметок просматривает назад `list` (по умолчанию 10) |

### `add-tags`
Добавляет теги в заметки через frontmatter и/или содержимое.

| Параметр | Тип | Обязателен | Описание |
|----------|-----|:----------:|----------|
| `vault` | string | ✓ | Имя vault |
| `files` | string[] | ✓ | Пути к заметкам относительно корня vault. **Все должны существовать** — если хотя бы одного нет, не изменяется ничего |
| `tags` | string[] | ✓ | Теги для добавления |
| `location` | string | | `frontmatter`, `content` или `both` (по умолчанию). Учтите, что `both` помещает тег в заметку **дважды** |
| `normalize` | boolean | | Нормализовать формат тегов (по умолчанию: `true`) |
| `position` | string | | `start` или `end` (по умолчанию) — куда попадёт тег в содержимом |

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

- [Rust](https://rustup.rs/) (stable; MSRV 1.88)
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

# Если сервер был запущен с --log-file, укажите тот же путь и команде `logs`,
# иначе она покажет лог по умолчанию, пока ваш пишется в другое место.
npx obsidian-mcp-rs logs --log-file /tmp/mcp-debug.log
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
          ├── ObsidianHandler → 15 реализаций MCP-инструментов
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
