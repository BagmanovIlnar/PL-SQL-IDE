# План разработки кроссплатформенной IDE для Oracle и PostgreSQL

## 1. Цель продукта

Создать самостоятельную кроссплатформенную IDE для разработки и сопровождения Oracle PL/SQL и PostgreSQL SQL/PL/pgSQL на Windows, Linux и macOS.

Продукт ориентируется на основные рабочие сценарии PL/SQL Developer, но не копирует его название, интерфейс, графику или иные защищённые элементы.

Основной приоритет:

1. Oracle и PostgreSQL входят в один MVP.
2. Надёжное выполнение SQL, PL/SQL и PL/pgSQL.
3. Безопасная работа с транзакциями и несколькими сессиями.
4. Удобный редактор на базе iced, навигация по объектам и просмотр результатов.
5. Общий интерфейс приложения при раздельной реализации диалектов и возможностей СУБД.

## 2. Технологическое решение

### Принятое решение

- Backend и GUI: Rust.
- Desktop UI: iced 0.14.
- Доп. виджеты: iced_aw при необходимости (tabs, menus).
- Редактор: `iced::widget::text_editor` + feature `highlighter`.
- PostgreSQL: `sqlx` с закреплённым `PoolConnection<Postgres>` за worksheet.
- Oracle: crate `oracle` на базе ODPI-C, feature `oracle`.
- Асинхронное ядро: Tokio через iced `Task`.
- Конфигурация: `serde`, `directories`.
- Секреты: системный keyring.

### Редактор: что делаем сами

`text_editor` — базовая основа. В MVP поверх него поэтапно добавляются:

- номера строк / gutter;
- search/replace;
- autocomplete и hover popup;
- diagnostics underline;
- оптимизация больших файлов.

Это осознанный trade-off: нативный Rust GUI вместо webview/Monaco.

### ADR

- `docs/adr/0001-gui-and-editor.md` — iced, не Tauri.
- `docs/adr/0002-oracle-client.md` — user-installed Oracle Client.

## 3. Архитектура

```text
UI (iced)
  ├── Connection Manager
  ├── Object Browser
  ├── Worksheets (text_editor)
  ├── Result Grid
  └── Messages / DBMS Output / Explain Plan
          │
Rust Application Core
  ├── Workspace
  ├── Session Manager
  ├── Execution Service
  ├── Script Splitter
  ├── Metadata Service
  ├── Secret Store
  └── Recovery Store
          │
Database Adapters
  ├── Oracle Adapter → session-affine blocking workers
  └── PostgreSQL Adapter → session-affine async tasks
```

Каждая worksheet закрепляется за одной физической сессией СУБД.

## 4. Основные типы и интерфейсы

Минимальный набор. Новые типы создаются только при отдельной ответственности.

### `AppState`

Корневое состояние iced-приложения: профили, workspace, сессии, настройки.

### `ConnectionProfile`

DTO без секретов: id, name, descriptor/url, username, auth_mode, secret_id, timeouts, read_only.

### `SecretStore`

`load/save/delete` через OS keyring.

### `DbAdapter`

`kind()`, `open_session()`, `metadata_provider()`, `script_splitter()`.

Реализации: `OracleAdapter`, `PostgresAdapter`.

### `SessionManager`

Управляет жизненным циклом сессий и привязкой worksheet → session.

### `OracleSessionWorker` / `PostgresSessionTask`

Один worker/task владеет одним физическим соединением. Cancel идёт отдельным путём.

### `Worksheet`

Документ редактора, session_id, transaction_state, active_execution, result.

### `ScriptSplitter`

Диалектное разделение current/selection/script. Regex не является основным parser.

### `ExecutionService` / `ResultModel` / `MetadataProvider` / `MetadataCache` / `RecoveryStore`

Как в предыдущей редакции плана: выполнение, пакетные результаты, метаданные, аварийное восстановление без секретов.

## 5. Фазы разработки

Оценки для одного опытного разработчика.

### Фаза 0. Технологический gate — 3–5 недель

- iced окно на 3 ОС;
- editor spike: 100k строк;
- PostgreSQL connect/query/cancel/timeout;
- Oracle connect/query/cancel/timeout за feature-flag;
- ADR по GUI и Oracle Client;
- CI matrix.

Критерии: сборка на 3 ОС, долгий запрос отменяется без зависания UI, нет блокирующих рисков по iced/Oracle Client.

### Фаза 1. Execution core — 10–14 недель

Независимые сессии, splitter, binds, commit/rollback, cancel, streaming results, DBMS_OUTPUT / notices.

### Фаза 2. Главное окно, меню File и workspace — 6–9 недель

Раскладка главного окна (3 области):

1. Верх — меню.
2. Лево — сайдбар: блок **Connections** сверху, блок **Objects** снизу.
3. Право — вкладки worksheets + редактор на всю оставшуюся область.

Нижняя панель Results/Messages появляется позже (Фаза 1/3, после execution core), не в каркасе окна.

#### File → New

- Program Window → blank, function, java source, package, package body, package specification, procedure, trigger, type, type body, type specification
- Test Window (анонимный блок)
- SQL Window
- Command Window
- Explain Plan Window

#### File → Open

- Program File / Test Script / SQL Script / Command File

#### File → Save / Save As / Exit

Также: вкладки, line numbers, search/replace, highlighter, recovery, shortcuts.

### Фаза 3. Metadata workflow — 10–14 недель

Object browser, DDL/source, compile/errors, View Data, Explain Plan для обеих СУБД.

### Фаза 4. Completion / hover / navigation — 5–8 недель

### Фаза 5. Security, packaging, beta — 5–7 недель

### Фаза 6. Паритет диалектов и стабилизация — 4–6 недель

## 6. Оценка сроков

- Технологический prototype: 1–2 месяца.
- Oracle + PostgreSQL MVP: 10–16 месяцев для одного опытного разработчика.
- Production-ready с подписанными релизами: 14–20 месяцев.
- Широкий аналог коммерческой IDE: ориентировочно 12–30 человеко-лет.

## 7. Definition of Done для MVP

- Устанавливается на Windows, Linux, macOS.
- Безопасные профили подключений.
- Независимые сессии worksheet.
- Execute / cancel / commit / rollback.
- PL/SQL и PL/pgSQL blocks, параметры, DBMS_OUTPUT / notices.
- Ленивое дерево объектов, source/DDL/diagnostics.
- Редактор закрывает ежедневные операции и recovery.
- Секреты не попадают в config и логи.
- Известные ограничения редактора iced перечислены в release notes.
