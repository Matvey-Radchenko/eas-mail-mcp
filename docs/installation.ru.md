# Установка EAS Mail MCP

Поддерживаются macOS 14+ на Apple Silicon и Intel. Windows и Linux не входят в
релиз `0.2.0`.

## Установка через npm

```bash
npm install -g eas-mail-mcp@next
eas-mail-mcp --version
eas-mail-mcp native-path
```

Npm нужен для установки и запуска административных CLI-команд. При настройке
клиента записывается прямой путь к Rust-бинарнику, поэтому Node.js не остаётся в
фоновом MCP-процессе. Пакет не использует `install` или `postinstall` scripts.

## Настройка

Запустите единый мастер:

```bash
eas-mail-mcp setup
```

Если команда получила готовый профиль, его можно импортировать заранее:

```bash
eas-mail-mcp profile import ./team-profile.toml
eas-mail-mcp setup
```

Профиль содержит адрес EAS-сервера, разрешённые почтовые домены и параметры TLS,
но не пароль и не данные конкретной учётной записи. Пароль вводится в закрытом
prompt и сохраняется только в macOS Keychain. Запись по умолчанию выключена.

Локальные файлы:

```text
~/Library/Application Support/EAS Mail MCP/profiles.toml
~/Library/Application Support/EAS Mail MCP/config.toml
```

## Подключение клиентов

```bash
eas-mail-mcp client configure codex
eas-mail-mcp client configure claude
eas-mail-mcp client configure opencode
```

Перед изменением пользовательских конфигов создаётся backup. Имя и версия
клиента сохраняются только для диагностики и не блокируют настройку. Команда
регистрирует MCP и удаляет устаревшие `approve`/`allow`-правила, созданные
предыдущими beta-сборками. Write tool выполняется сразу после вызова, если запись
включена для аккаунта.

Проверка после настройки:

```bash
eas-mail-mcp account list
eas-mail-mcp profile list
eas-mail-mcp doctor
```

## Обновление и удаление

Обновление не удаляет локальные профили, аккаунты или Keychain:

```bash
npm install -g eas-mail-mcp@next
```

Удаление npm-пакета также оставляет пользовательские данные на месте:

```bash
npm uninstall -g eas-mail-mcp
```

Старый локальный `tar.gz`-установщик остаётся только резервным путём для пилота.
