# Codex Harness: карта официальной документации

Цель репозитория — стать переносимой основой глобальной настройки Codex CLI для разработки в других проектах, используя лучшие подходы из `opencode-kit`. Приоритеты — результативность, качество с фокусом на отсутствие P0/P1 и скорость получения полного, проверенного результата. Подтверждённые договорённости и критерий завершения работы записаны в [философии и решениях проекта](project-decisions.md).

Основой выбран официальный Codex CLI; собственный оркестратор или изменения CLI рассматриваем только при доказанной недостаточности штатных расширений. Рабочие принципы перенесены; установщик прямых ссылок и launcher реализованы в [link-global-codex-kit](../openspec/changes/archive/2026-09-06-link-global-codex-kit/proposal.md). Текущий статус приёмки записан в [решениях проекта](project-decisions.md#воспроизводимое-развёртывание-из-репозитория).

- [Философия и решения проекта](project-decisions.md) — постоянная запись пользовательских договорённостей, включая OpenSpec и завершение всей спеки.
- [Рабочие принципы](../global/principles-of-work.md) — полный переносимый текст для постоянного контекста.
- [Адаптация принципов](principles-port.md) — сверка с оригиналом и согласованные изменения.
- [Глобальное подключение](global-instructions.md) — действующая ссылка, проверка загрузки, обновление и откат.
- [Установка набора](installation.md) — зависимости, конфигурация, ссылки, проверка, обновление и отключение.
- [Диагностика источников](source-diagnostics.md) — глобальный отчёт о ссылках, слоях настроек и конфликтах skills без model calls.
- [Подписки и внешние модели](subscription-models.md) — OpenCodex, OAuth Grok, назначение моделей ролям и текущий статус приёмки.
- [Делегирование по уровням](agent-delegation.md) — приоритет Grok, резерв Astra, редкая max-консультация и проверка полной стоимости результата.
- [Проверки потребителя](linked-kit-probes.md) — фактическое чтение профилей, skills и агентов установленным CLI.
- [Приёмка набора](linked-kit-verification.md) — сверка сценариев, результаты тестов, реальная активация и очистка.
- [Практики скорости и качества](best-practices.md).
- [Карта переноса из opencode-kit](opencode-kit-map.md).
- [Аудит дополнительных заимствований](opencode-kit-audit.md) — приоритеты, конкретные исходники, проверки и цена адаптации; предложения от 2026-09-07.
- [Глобальное MCP/LSP-подключение](code-tools.md) — выбранные инструменты, прямое чтение исходников, состояние реализации и доказательства; [спека и задачи](../openspec/changes/archive/2026-09-06-connect-global-mcp-lsp/proposal.md).
- [Приёмка MCP/LSP](code-tools-verification.md) — соответствие требованиям, языковая матрица, глобальные consumers, обновления и ограничения проверок.
- Ниже — 56 официальных страниц, сгруппированных по задачам.

## Как пользоваться

1. Найти нужную возможность в таблице маршрутов.
2. Открыть соответствующий официальный контракт и только нужные разделы.
3. Перед реализацией проверить текущую версию CLI, применимость к CLI и статус возможности.
4. После изменения проверить поведение через реально используемую точку входа.

Этот каталог — навигация и краткие выводы, а не полный локальный архив документации. Ссылки на дополнительные официальные индексы позволяют продолжить поиск за пределами выбранных тем. Примеры из статей и Cookbook следует адаптировать к принятому объёму задачи.

## Быстрые маршруты

| Хотим сделать | С чего начать |
| --- | --- |
| Подключать набор к любому проекту | [Установка](installation.md) → Config basics → Advanced configuration → Environment variables |
| Настроить глобальные и проектные инструкции | [Наше подключение](global-instructions.md) → AGENTS.md → Build skills |
| Выбирать профиль модели и reasoning | Models → Speed → Advanced configuration |
| Создать специализированного агента | Subagents; для собственного приложения — Agents SDK |
| Создать повторяемый workflow | Build skills → Testing Agent Skills Systematically with Evals |
| Упаковать несколько возможностей | Build plugins → Package your plugin → Connect and test |
| Подключить MCP | MCP in Codex; для собственного сервера — Define tools → Build an MCP server |
| Получать свежую официальную документацию | OpenAI Docs MCP и индексы в конце страницы |
| Добавить навигацию по символам, диагностику, LSP | MCP in Codex → кейс JetBrains MCP → исследование конкретного сервера |
| Реагировать на начало/конец работы и tool calls | Hooks; при упаковке — bundled lifecycle hooks |
| Улучшить длительную работу и память | Using Goals → Memories → настройки контекста; API Compaction — для собственного runtime |
| Проверять, что улучшение приносит пользу | Практики скорости и качества → eval-skills |
| Запускать работу из скрипта или CI | Non-interactive mode → Codex SDK → GitHub Action |
| Написать собственный клиент | App Server → Open source components |
| Разрабатывать сам Codex CLI | Open source components → официальный репозиторий openai/codex |
| Разобраться с выполнением на Windows | Windows sandbox → CLI reference; WSL при необходимости Linux-окружения |

## Проверенный срез

Дата исследования: **2026-09-06**. Локально наблюдался **codex-cli 0.153.4** через `codex --version`; также проверены `codex --help`, `codex plugin --help` и `codex features list`. Это сведения о машине исследования, а не требование к установке.

Страницы каталога открыты через браузер или их Markdown-представления; подробно прочитаны относящиеся к задаче разделы. Это не полный аудит всех параметров, реализаций или ссылок внутри страниц.

Особенности, которые стоит учитывать при следующем обращении:

- Старый раздел `developers.openai.com/codex/` перенаправляет на документацию в `learn.chatgpt.com`. Для поиска есть [официальный индекс](https://learn.chatgpt.com/docs/llms.txt).
- Большинство страниц имеют вариант с суффиксом `.md`. У Best practices HTML-страница доступна, а указанный индексом Markdown-адрес при проверке вернул 404: использовать ссылку на HTML.
- Текущая документация и установленный бинарник могут расходиться. Например, таблица [Config basics](https://learn.chatgpt.com/docs/config-file/config-basic) помечала `memories` как experimental, а локальный `features list` — stable, disabled. Перед использованием смотреть тематическую страницу и поведение целевой версии.
- [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk) помечает `codex mcp-server` deprecated, хотя команда ещё есть в локальном help. Это отдельный вопрос от поддержки подключения внешних MCP-серверов к Codex.
- [Evaluation best practices](https://developers.openai.com/api/docs/guides/evaluation-best-practices) содержит уведомление о завершении работы Evals platform. Методика оценки полезна; перед выбором сервиса проверки нужно заново открыть его актуальную документацию.
- В просмотренной документации CLI и конфигурации не найден подтверждённый встроенный механизм регистрации LSP-серверов. Возможность остаётся предметом исследования; это не доказательство отсутствия во всех версиях. MCP с возможностями IDE/языкового сервера — отдельный путь интеграции.

## Каталог

### Ориентация и актуальность

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Обзор Codex CLI](https://learn.chatgpt.com/docs/codex/cli) | Установка и основные сценарии работы из терминала. | CLI |
| [Best practices](https://learn.chatgpt.com/guides/best-practices) | Общие рекомендации: контекст, инструкции, планирование, проверка, skills и MCP. | CLI / IDE / app |
| [Prompting](https://learn.chatgpt.com/docs/prompting) | Раздел Prompting Codex: воспроизведение ошибок, контекст, ограничения и проверка результата. | Несколько продуктов; читать раздел Codex |
| [Model guidance / latest-model](https://developers.openai.com/api/docs/guides/latest-model) | Поведение конкретных моделей, автономность, инструкции, делегирование и соразмерность проверок. | API / модель; применимость к CLI проверять |
| [Models](https://learn.chatgpt.com/docs/models) | Выбор модели в Codex и ограничения доступности. | Codex; зависит от поверхности и аккаунта |
| [Feature maturity](https://learn.chatgpt.com/docs/feature-maturity) | Значение статусов stable, beta, experimental и under development. | Codex / ChatGPT |

### Глобальная настройка и загрузка контекста

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Config basics](https://learn.chatgpt.com/docs/config-file/config-basic) | Глобальные и проектные конфиги, доверие проекту и порядок приоритетов. | CLI / IDE |
| [Advanced configuration](https://learn.chatgpt.com/docs/config-file/config-advanced) | Профили, CODEX_HOME, overrides, корень проекта, окружение команд и наблюдаемость. | Локальные клиенты |
| [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) | Точные поля config.toml и requirements.toml; ссылка на JSON Schema. | Контракт конфигурации |
| [Sample configuration](https://learn.chatgpt.com/docs/config-file/config-sample) | Пример для сверки формата; выбирать нужные поля по reference. | Пример, не готовый конфиг этого репозитория |
| [Environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables) | CODEX_HOME и другие пути, установка и диагностика. | Локальные клиенты |
| [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md) | Глобальные, проектные и вложенные инструкции, override и лимиты загрузки. | Инструкции Codex |
| [Customization overview](https://learn.chatgpt.com/docs/customization/overview) | Выбор между AGENTS.md, skill, MCP и subagent. | Архитектура расширения |

### Skills, агенты и плагины

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Build skills](https://learn.chatgpt.com/docs/build-skills) | SKILL.md, discovery, progressive disclosure, пути, триггеры и тестирование. | Локальные skills и распространение |
| [Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents) | Делегирование, контекст, ограничения и TOML-файлы пользовательских агентов. | Codex CLI / IDE / app |
| [Build plugins](https://learn.chatgpt.com/docs/build-plugins) | Обзор пакета, manifest и путь от личного skill к плагину. | ChatGPT / Codex |
| [Package your plugin](https://developers.openai.com/plugins/build/plugins) | plugin.json, marketplace.json, локальная установка, bundled MCP и hooks. | Контракт упаковки |
| [Use plugins](https://learn.chatgpt.com/docs/plugins) | Установка, включение и использование плагинов, включая CLI. | Проверять раздел нужного клиента |
| [Connect and test your plugin](https://developers.openai.com/plugins/deploy/connect-chatgpt) | Проверка отдельных возможностей и установленного пакета. | Шаги UI могут относиться к ChatGPT |

### MCP, работа с кодом и hooks

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [MCP in Codex](https://learn.chatgpt.com/docs/extend/mcp) | STDIO и Streamable HTTP, авторизация, фильтры tools, таймауты и конфигурация. | Клиентская интеграция |
| [OpenAI Docs MCP](https://developers.openai.com/learn/docs-mcp) | Официальный сервер поиска документации OpenAI; кандидат для будущего подключения. | Документация; сейчас не устанавливался |
| [Build an MCP server](https://developers.openai.com/plugins/build/mcp-server) | Разработка сервера, tools, ответы и расширения для skills. | Серверная разработка |
| [Define tools](https://developers.openai.com/plugins/plan/tools) | Входы, выходы, описания для выбора tools и границы действий. | Проектирование MCP tools |
| [Hooks](https://learn.chatgpt.com/docs/hooks) | События жизненного цикла, источники, trust, входы/выходы и семантика продолжения. | Контракт Codex hooks |
| [Codex + JetBrains MCP](https://developers.openai.com/blog/skyscanner-codex-jetbrains-mcp) | Опубликованный OpenAI опыт подключения диагностики и средств IDE через MCP. | Кейс интеграции, не контракт встроенного LSP |

### Скорость, качество и длинные задачи

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Speed](https://learn.chatgpt.com/docs/agent-configuration/speed) | Fast mode, применимость и компромиссы скорости и расхода. | Codex; тарифы проверять заново |
| [Testing Agent Skills Systematically with Evals](https://developers.openai.com/blog/eval-skills) | Проверка триггеров, результата, лишних команд, артефактов и токенов. | Методика оценки skills |
| [Code review](https://learn.chatgpt.com/docs/code-review) | Область ревью и работа с результатами проверки. | Выбирать раздел CLI |
| [Using Goals in Codex](https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex) | Устойчивые цели, критерий завершения и ограничения длительной работы. | Cookbook; не активирует Goal |
| [Iterating development workflows with Codex](https://developers.openai.com/cookbook/examples/codex/iterating-development-workflows-with-codex) | Пример развития repo harness, skills и ретроспектив. | Авторский workflow; дополнительные файлы не обязательны для Codex |
| [Run long horizon tasks with Codex](https://developers.openai.com/blog/run-long-horizon-tasks-with-codex) | Эксперимент с длительной разработкой, памятью и проверкой этапов. | Опыт на конкретной модели, не гарантия результата |
| [Memories](https://learn.chatgpt.com/docs/customization/memories) | Локальная память, хранение, включение и проверка. | Вспомогательный контекст |
| [Worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees) | Изоляция параллельной работы и перенос между окружениями. | Часть управления относится к app; Git доступен отдельно |

### Автоматизация и разработка вокруг CLI

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [CLI command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli) | Флаги и команды exec, review, plugin, mcp, doctor и другие. | Сверять с установленным --help |
| [Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode) | codex exec, JSONL, output schema, stdin и продолжение сессий. | Скрипты / CI |
| [Codex SDK](https://learn.chatgpt.com/docs/codex-sdk) | Программное управление Codex; TypeScript и Python SDK. | Автоматизация поверх Codex |
| [App Server](https://learn.chatgpt.com/docs/app-server) | Протокол для клиентов: threads, turns, события, approvals и история. | Глубокая интеграция |
| [Codex GitHub Action](https://learn.chatgpt.com/docs/github-action) | Вызов Codex в GitHub Actions, параметры и результаты. | Опциональный CI |
| [Codex as a platform](https://developers.openai.com/blog/codex-as-a-platform) | Выбор слоя интеграции и устройство открытого harness. | Архитектурное объяснение |
| [Open source components](https://learn.chatgpt.com/docs/open-source) | Официальная карта репозиториев CLI, SDK, app-server, skills и plugins. | Навигация по исходникам OpenAI |

### Доступ, Windows и окружение

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Approvals and security](https://learn.chatgpt.com/docs/agent-approvals-security) | Различия approval policy, sandbox и сетевых ограничений. | Конфигурация выполнения |
| [Rules](https://learn.chatgpt.com/docs/agent-configuration/rules) | Формат правил команд и их проверка. | В документации помечено experimental |
| [Permission profiles](https://learn.chatgpt.com/docs/permissions) | Профили файлового и сетевого доступа; взаимодействие со старыми настройками. | В документации помечено beta |
| [Windows sandbox](https://learn.chatgpt.com/docs/windows/windows-sandbox) | Нативная работа на Windows и диагностика sandbox. | Windows CLI / IDE / app |
| [WSL](https://learn.chatgpt.com/docs/windows/wsl) | Linux-окружение, пути и типичные проблемы производительности. | Опциональное окружение |

### API и собственные агенты — дополнительный маршрут

| Источник | Когда открывать | Применимость |
| --- | --- | --- |
| [Agents SDK quickstart](https://developers.openai.com/api/docs/guides/agents/quickstart) | Начало разработки агента с SDK. | Собственный API runtime |
| [Orchestration and handoffs](https://developers.openai.com/api/docs/guides/agents/orchestration) | Handoffs и agents-as-tools, ответственность за результат. | Agents SDK |
| [Integrations and observability](https://developers.openai.com/api/docs/guides/agents/integrations-observability) | MCP и трассировка в SDK. | Agents SDK |
| [Latency optimization](https://developers.openai.com/api/docs/guides/latency-optimization) | Меньше запросов и лишней генерации, параллельная независимая работа. | Принципы / собственный API runtime |
| [Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) | Префиксы, жизненный цикл кэша и отличия между моделями. | API; не готовые ключи config.toml |
| [Compaction](https://developers.openai.com/api/docs/guides/compaction) | Сжатие состояния длинных взаимодействий. | Responses API |
| [Tool search](https://developers.openai.com/api/docs/guides/tools-tool-search) | Загрузка определений инструментов по необходимости. | Responses API |
| [Prompt engineering](https://developers.openai.com/api/docs/guides/prompt-engineering) | Версионирование prompts, роли и полезный контекст. | API |
| [Codex Prompting Guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide) | Рекомендации для собственного harness с Codex-моделью. | Руководство для gpt-5.3-codex через API |
| [Evaluate agent workflows](https://developers.openai.com/api/docs/guides/agent-evals) | Навигация по трассам и оценке поведения агента. | API; доступность сервисов перепроверять |
| [Evaluation best practices](https://developers.openai.com/api/docs/guides/evaluation-best-practices) | Методика проверки вариативных систем и предупреждение о deprecation Evals platform. | Методика; сервис имеет отдельный жизненный цикл |

## Официальные исходники и схемы

[Open source components](https://learn.chatgpt.com/docs/open-source) подтверждает принадлежность перечисленных репозиториев OpenAI. В этом исследовании использовалась эта карта; содержимое исходников и конкретные release tags отдельно не аудировались.

| Ресурс | Для чего |
| --- | --- |
| [openai/codex](https://github.com/openai/codex) | Исходники CLI, инструкции разработки и сборки, актуальные releases и изменения |
| [Codex SDK sources](https://github.com/openai/codex/tree/main/sdk) | Контракты и примеры программного управления |
| [App Server sources](https://github.com/openai/codex/tree/main/codex-rs/app-server) | Протокол, реализация и генерация схем |
| [openai/skills](https://github.com/openai/skills) | Официальные примеры skills |
| [openai/plugins](https://github.com/openai/plugins) | Официальные примеры плагинов |
| [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) | Ссылка на текущую JSON Schema конфигурации |
| [Hooks: schemas](https://learn.chatgpt.com/docs/hooks#schemas) | Маршрут к схемам hooks и оговорка о различиях main и release |

Для изменения самого CLI сначала читать инструкции разработки выбранного checkout. Возможность в ветке `main` сама по себе не доказывает наличие в установленном релизе.

## Индексы для дальнейшего поиска

- [Codex / ChatGPT documentation index](https://learn.chatgpt.com/docs/llms.txt).
- [OpenAI Developers index](https://developers.openai.com/llms.txt).
- [API index](https://developers.openai.com/api/llms.txt) → [guides index](https://developers.openai.com/api/docs/llms.txt).
- [Plugin builder index](https://developers.openai.com/plugins/llms.txt).
- [Developer blog index](https://developers.openai.com/blog/llms.txt).
- [Cookbook index](https://developers.openai.com/cookbook/llms.txt).

При добавлении источника указывать владельца, назначение и применимость. Для сторонних MCP/LSP использовать документацию разработчика конкретного компонента и спецификацию протокола; явно обозначать, что это другой поставщик. Публичный репозиторий сам по себе не делает материал официальной рекомендацией OpenAI.

## Что ещё нужно решить

- Нужна ли следующая поддержка WSL, Linux или macOS после Windows native.
- Какие предложения из [аудита opencode-kit](opencode-kit-audit.md) выбрать для следующей реализации после MCP/LSP, подписок и делегирования.
- Как измерить выигрыш новых возможностей на сопоставимых задачах; политика выбора уровней и расхода уже [согласована](agent-delegation.md).

Эти решения не считаются принятыми по наличию ссылки или примера в каталоге.
