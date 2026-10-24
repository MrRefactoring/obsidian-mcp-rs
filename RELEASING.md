# Подготовка релиза

## Доступ npm через OIDC

Публикация восьми пакетов использует GitHub Actions Trusted Publishing.
Постоянный `NPM_TOKEN` для workflow не нужен. Параметры доверенной связи для
каждого пакета:

| Поле npm | Значение |
|---|---|
| Publisher | GitHub Actions |
| Organization or user | `MrRefactoring` |
| Repository | `obsidian-mcp-rs` |
| Workflow filename | `release.yml` |
| Environment name | пусто |
| Allow npm publish | включено |
| Allow npm dist-tag | выключено |

Связь настраивается в Settings → Trusted Publisher для `obsidian-mcp-rs` и
семи пакетов `@obsidian-mcp-rs/`: `darwin-arm64`, `darwin-x64`, `linux-arm64`,
`linux-x64`, `linux-x64-musl`, `win32-arm64`, `win32-x64`.

Workflow выдаёт npm-задаче `id-token: write` и устанавливает npm CLI 11 под Node.js 22.
Для настройки и требований CLI см. [документацию npm](https://docs.npmjs.com/trusted-publishers/).

Новая связь истекает, если первая успешная публикация не выполнена за два дня.
Поэтому сохраняйте настройки, когда проверенный релиз готов к выпуску.
`npm publish --dry-run`, `npm whoami` и повторный запуск, который пропустил уже
опубликованные версии, не подтверждают работу публикации через OIDC.

При переходе с токена сначала проверьте реальную OIDC-публикацию всех восьми пакетов.
После этого удалите секрет GitHub `NPM_TOKEN`, отзовите ненужный токен npm и в настройках
пакетов запретите публикацию через bypass-2FA токены. Не удаляйте `CARGO_REGISTRY_TOKEN`:
он нужен независимой задаче crates.io.

## Версии и проверки

Подготовьте выпуск в отдельной ветке:

```bash
./scripts/prepare-release.sh 0.8.1
```

Скрипт согласует Cargo, восемь npm-пакетов, `optionalDependencies` и lockfile.
Заполните созданный раздел `CHANGELOG.md`, не добавляя `[Unreleased]`.
Проверьте diff и выполните проверки из [CONTRIBUTING.md](CONTRIBUTING.md).
Дождитесь полной матрицы CI на итоговом коммите PR.

## Публикация и проверка результата

После слияния PR тег должен указывать на коммит из `master`, версия которого
совпадает с Cargo и всеми npm-пакетами. Workflow проверяет это до сборок.

GitHub Release должен содержать семь бинарников с прежними именами, `checksums.txt`
и provenance attestations. Проверьте версию crate на crates.io, версии и `latest`
всех восьми npm-пакетов, SRI tarball и соответствие SHA-256 упакованных бинарников
файлу `checksums.txt`.

Запустите опубликованный пакет под Node.js 22 с отдельным npm cache и тестовым vault.
Проверьте `--version`, MCP initialize, tools/list и read-note, а также отсутствие
диагностики в stdout. Не запускайте тестовый `install` против личных конфигураций.

Для повторного запуска после исправления сбоя используйте failed jobs существующего
workflow. Уже опубликованные версии пропускаются. Артефакты Actions сохраняются
один день; если они истекли, потребуется повторная сборка.

Автообновление начинает распространять выпуск через 48 часов после публикации
GitHub Release. Если выпуск нужно отозвать, отметьте его как prerelease:
установленные серверы следуют `releases/latest` в обе стороны.
