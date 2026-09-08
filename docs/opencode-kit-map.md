# Карта переноса из opencode-kit

Текущий выбор после оценки расхода 2026-09-08 — [четыре MCP, явный Python через
Serena и выключенные хуки](evidence/subscription-efficiency.md). Описанная ниже
языковая и автоматическая поставка 2026-09-06 является исторической, а не
требованием её восстановить.

[Карта документации](README.md) · [Практики скорости и качества](best-practices.md)

Срез исследования: 2026-09-06. Источник сравнения — локальный соседний checkout `../opencode-kit`. Пути в колонке источника относятся к нему. Это выборочная карта возможностей по прочитанным файлам, а не полный аудит реализации.

Дополнено 2026-09-07: [аудит дополнительных заимствований](opencode-kit-audit.md) сверяет кандидатов с текущей поставкой, содержит проверку выбранных исходных helpers и приоритеты дальнейшей адаптации. Подписки и [универсальные уровни агентов](agent-delegation.md) уже подключены; ниже сохраняется общая карта механизмов, а не список невыполненных задач.

Историческая поставка 2026-09-06 выбрана пользователем и описана в [connect-global-mcp-lsp](../openspec/changes/archive/2026-09-06-connect-global-mcp-lsp/proposal.md): четыре MCP, LSP для Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake и Bash, переиспользование/обновление общих установок и автоматическая диагностика после правок; YAML, QML, HTML и CSS подключаются при наличии готовой совместимой поддержки. Реальные глобальные вызовы MCP и нативная автодиагностика четырнадцати языков плюс HTML/CSS проверены; YAML/QML недоступны. Все 45 задач выполнены, change архивирован 2026-09-06; доказательства приёмки и ограничения — в [отчёте](code-tools-verification.md), нормативный объём — в [основных спеках](../openspec/specs/), технические решения — в архивном design. Упоминание Serena и Codebase Memory в таблице ниже отражает переносимый template: машинная конфигурация дополнительно содержит Graphify и Nuphus.

Цель, заданная пользователем: глобально применимый набор для официального Codex CLI, который использует лучшие подходы `opencode-kit` и улучшает результативность, качество и скорость разработки в других репозиториях. Подтверждённые принципы, граница расширения CLI и критерий завершения через OpenSpec записаны в [решениях проекта](project-decisions.md).

## Наблюдения

- `README.md` описывает глобальную установку, выборочные профили `core`, `core-beads`, `all`, переносимые инструменты и адаптеры проектов.
- `global/principles-of-work.md` формулирует приоритеты результата, качества, автономности, быстрого получения обратной связи, простоты и улучшений по наблюдениям.
- `global/AGENTS.md` занимает 61 919 байт, `global/principles-of-work.md` — 12 844 байта в исследованном checkout.
- `global/opencode.json.template` содержит MCP Serena и Codebase Memory, OpenCode plugins, настройки compaction, watcher и tool output.
- `docs/project-memory.md` и `docs/kaizen.md` описывают отдельные механизмы памяти и межпроектного сбора сигналов улучшения.

Эти факты описывают исходный kit. Рабочая философия уже адаптирована в [global/principles-of-work.md](../global/principles-of-work.md); [сравнение](principles-port.md) объясняет перенос. MCP/LSP подключены в текущей реализации с отдельной приёмкой, остальные перечисленные возможности сохраняют свой отдельный статус.

## Возможности и точки адаптации

| В opencode-kit | Возможный путь в Codex | Что выяснить перед переносом |
| --- | --- | --- |
| `global/principles-of-work.md`, `docs/token-economy.md` | Короткие глобальные инструкции и тематические skills | Какие правила нужны постоянно; какие сокращают реальную задержку |
| `global/AGENTS.md` | Глобальный `AGENTS.md`, ссылки на подробности | Порядок загрузки, ограничение размера, конфликты с проектными правилами |
| `tools/install-opencode-global.ts` | Установка управляемых файлов в поддерживаемые Codex paths | Разделение исходников, авторизации и состояния; обновление и откат |
| `profiles/core.json` и другие install manifests | Выбор состава пакета; отдельно — config profiles Codex | Профиль установки kit и профиль настроек модели выполняют разные задачи |
| `global/skills/` | `SKILL.md` в поддерживаемых путях либо в plugins | Триггеры, зависимости, ссылки, OpenCode-specific команды |
| `global/agents/` | Пользовательские TOML agents | Схема Codex, роли, модель, доступ и стоимость координации |
| `global/commands/` | Skills и поддерживаемые CLI-команды | Как пользователь будет вызывать действие; совместимость slash commands |
| `global/opencode.json.template`: MCP | `mcp_servers` в Codex config или bundled MCP | Запуск, cwd, транспорт, tool selection, таймауты и поддержка ОС |
| Serena / Codebase Memory | Кандидаты MCP для навигации и контекста | Документация владельцев, актуальная совместимость, польза относительно текущих средств |
| `global/plugin/`, `global/plugins/`, `global/extensions/` | Plugins + hooks/MCP; при необходимости SDK/App Server | У каждого расширения свой контракт; формат TypeScript plugin OpenCode напрямую не переносится |
| Compaction prompt и session completion guard | Нативное управление контекстом, Goals, Stop hooks | Эквивалентность продолжения, остановки и восстановления состояния |
| `docs/project-memory.md` | Нативные Memories либо отдельное расширение | Изоляция проектов, актуальность, управляемость и доступность |
| `docs/kaizen.md` | Сбор наблюдений и оценка улучшений через skills/tools/hooks | Минимальный полезный цикл обратной связи без создания лишнего процесса |
| `templates/project/`, `tools/init-project.ts` | Опциональный bootstrap и проектные инструкции/адаптер | Что работает глобально, что должно принадлежать конкретному проекту |
| `tools/doctor.ts`, inventories и proofs | Нативная диагностика плюс проверки подключённых возможностей | Реальная загрузка в чужом проекте, корректные прямые ссылки на checkout kit и диагностика их доступности |

Контракты назначения: [configuration](https://learn.chatgpt.com/docs/config-file/config-basic), [profiles и state](https://learn.chatgpt.com/docs/config-file/config-advanced), [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md), [skills](https://learn.chatgpt.com/docs/build-skills), [subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), [MCP](https://learn.chatgpt.com/docs/extend/mcp), [plugin packaging](https://developers.openai.com/plugins/build/plugins), [hooks](https://learn.chatgpt.com/docs/hooks).

## Предлагаемая модель глобального подключения

Пользователь подтвердил прямое чтение глобальной конфигурации, агентов и skills из этого репозитория на новом ПК, без копирования артефактов или сборки их дубликатов. `install.ps1` реализует регистрацию через ссылки и нативный профиль; точный состав и границы описаны в [инструкции установки](installation.md), статус — в [решениях проекта](project-decisions.md#воспроизводимое-развёртывание-из-репозитория). Ниже — общая схема подключения:

```text
codex-harness checkout
  docs + shared instructions + selected capabilities
                       |
                       v
            register paths / file links
                       |
          +------------+------------+
          |                         |
          v                         v
   linked configuration      linked skills / agents
          |                         |
          +------------+------------+
                       |
                       v
            Codex in a target project
                       +
          project instructions and tools
```

В репозитории хранятся управляемые исходники и документация. Установщик регистрирует поддерживаемые пути и файловые ссылки на эти исходники, сохраняя личные и проектные настройки в их области ответственности. Содержимое артефактов остаётся единственным в checkout; копирование, генерация объединённого конфига и fallback на копии исключены.

[Environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables) описывает `CODEX_HOME` как корень конфигурации, авторизации, логов, сессий и другого состояния. Простое назначение checkout как `CODEX_HOME` требует отдельного решения о всём этом состоянии.

В текущей документации глобальные инструкции живут в Codex home, личные skills — в `$HOME/.agents/skills`, пользовательские агенты — в `~/.codex/agents/`; marketplace имеет собственные пути. Единого назначения каталога недостаточно считать установку всех классов артефактов доказанной. [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md), [Build skills](https://learn.chatgpt.com/docs/build-skills), [Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents), [Package your plugin](https://developers.openai.com/plugins/build/plugins)

Корневой `AGENTS.md` в `codex-harness` действует при работе над этим репозиторием. Рабочие принципы [подключены отдельно](global-instructions.md) через глобальный `AGENTS.md` пользователя; сам по себе файл в соседнем checkout в другие проекты не загружается. Реализованный установщик также связывает профиль, шесть OpenSpec skills и каталог агентов. Дополнительно поставлены MCP/LSP, hooks диагностики, подписки и универсальные уровни агентов; возможности без отдельного отчёта приёмки сохраняют исследовательский статус.

## Ограничения прямого переноса

1. Документация Codex указывает стандартный лимит цепочки инструкций 32 КиБ; исходный `global/AGENTS.md` уже больше. Требуются выбор содержания и проверка загрузки. [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
2. Codex plugin — пакет с manifest, skills и опциональными MCP/hooks. Поведение расширений OpenCode нужно сопоставлять по событиям и эффектам. [Package your plugin](https://developers.openai.com/plugins/build/plugins), [Hooks](https://learn.chatgpt.com/docs/hooks)
3. Profiles Codex описывают overlays конфигурации. Они сами по себе не заменяют выбор install manifest исходного kit. [Advanced configuration](https://learn.chatgpt.com/docs/config-file/config-advanced)
4. Наличие LSP, code graph или memory в исходном kit не доказывает поддержку в Codex. Подтверждённый маршрут исследования — MCP, включая средства IDE. [MCP](https://learn.chatgpt.com/docs/extend/mcp), [Кейс JetBrains](https://developers.openai.com/blog/skyscanner-codex-jetbrains-mcp)
5. Документация API, SDK, CLI и app может описывать разные настройки и жизненные циклы. Проверять выбранную точку исполнения. [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk), [App Server](https://learn.chatgpt.com/docs/app-server)

## Предлагаемое продолжение

Первичная глобальная загрузка skills, MCP/LSP и агентов уже проверена. Актуальные кандидаты, их ограничения и предлагаемый порядок находятся в [аудите от 2026-09-07](opencode-kit-audit.md). Следующий пакет следует выбирать по пользе для целевых проектов и проверять сравнением baseline/candidate. Поддержка других ОС остаётся отдельным вопросом.

Аудит не создаёт OpenSpec change и не фиксирует обязательный roadmap реализации.
