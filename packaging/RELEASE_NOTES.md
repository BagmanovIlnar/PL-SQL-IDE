## PLSQL IDE 0.0.1

Первый публичный релиз кроссплатформенной IDE для Oracle PL/SQL и PostgreSQL.

### Что внутри

- Оболочка iced: лента (File / Session / Settings / Help), Connections, Objects, вкладки worksheets
- SQL / PL/SQL редактор: подсветка, нумерация строк, автодополнение ключевых слов, breakpoints в gutter
- Подключения PostgreSQL и Oracle (оба в сборке по умолчанию; для Oracle в runtime нужен Instant Client)
- Настройки Oracle: Connection, Options, Compiler, Debugger
- Отладка PL/SQL: DBMS_DEBUG и DBMS_DEBUG_JDWP, панель инструментов, горячие клавиши
- Параметры bind / test window, i18n (EN / RU), светлая и тёмная тема
- Пустая рабочая область при старте, закрытие вкладок с запросом сохранения

### Пакеты

| Платформа | Файлы |
| --- | --- |
| Windows x86_64 | `.exe`, `.zip` |
| Linux x86_64 | `.tar.gz`, `.deb`, `.rpm` |
| macOS Apple Silicon (M) | `.zip` (`.app`), `.dmg` |

### Важно

- Релизные пакеты собраны с фичами по умолчанию (PostgreSQL + Oracle). Instant Client не бандлится — ставится отдельно для подключения к Oracle.
- macOS: приложение не нотаризовано — при первом запуске может понадобиться «Открыть» через контекстное меню.

### Требования

- Rust 1.90+ для сборки из исходников
- Linux: пакеты X11 / Wayland / xkbcommon (см. README)
