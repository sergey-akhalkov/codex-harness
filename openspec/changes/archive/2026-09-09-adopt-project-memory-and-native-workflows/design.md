## Context

Мотивация и подтверждённый объём — в [proposal.md](proposal.md). Срез исследования: **2026-09-07–08**, установленный **Codex CLI 0.153.4**, Windows / PowerShell. Обычный `codex` разрешается в установленный harness launcher, который выбирает файловый профиль `harness` и передаёт аргументы native CLI. Просмотренный `tools/launcher.psm1` сам slash-команды не обрабатывает.

В [наборе](../../../../global/kit.psd1) уже предусмотрены прямые подключения `.agents/skills/`, профиля и инструкций; [linked-global-kit](../../../specs/linked-global-kit/spec.md) требует работу вне checkout и сохраняет native persistence настроек. В [docs/project-decisions.md](../../../../docs/project-decisions.md) уже хранится постоянный контекст пользователя. `project-verification` и `reproduce-regression` присутствуют в текущих исходниках; их отдельная глобальная приёмка развивается в соседнем change. Здесь переиспользуем их контракты, не объявляя чужую работу завершённой.

### Проверенные контракты

| Возможность | Наблюдение | Граница доказательства |
| --- | --- | --- |
| Context management | Accepted Astra runtime pilot 2.1 and ordinary source-linked runtime, explicit false override and profile rollback 2.2 on CLI 0.153.4 | Separate global skill lifecycle acceptance passed; see the final evidence |
| Native memories | `memories=false`; official store is generated local state under Codex home (`~/.codex/memories/` by default) | Native memories remain excluded; Git project memory is the selected owner |
| `/btw`, `/side` | Зарегистрированы в native TUI 0.153.4 при `multi_agent_v2=false` и `true`; подробности ниже | Боковой диалог текущей пользовательской сессии не инспектировался |
| Worktrees | Можно использовать обычные команды Git в Windows CLI | Managed worktrees, Handoff и app local-environment относятся к desktop app |
| Structured exec | Native help содержит `--output-schema`, `--json`, `--output-last-message`, `--ephemeral`, `--sandbox` | Ни JSONL, ни exit 0 сами по себе не доказывают правильность результата |

Основания: [экспериментальный контекст Astra](https://learn.chatgpt.com/docs/models#experimental-context-management), [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference), [JSON schema](https://learn.chatgpt.com/docs/config-schema.json), [native memories](https://learn.chatgpt.com/docs/customization/memories), [CLI commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli), [app worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees), [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode). Прочитаны соответствующие страницы/Markdown, а локальные флаги сверены через help. Публичная документация может описывать более новую реализацию, чем установленная версия.

## Goals / Non-Goals

**Goals:** минимальные переносимые workflows с реальными consumers; выборочное восстановление полезных знаний; изоляция самостоятельных изменений; машинно проверяемая передача результата. Польза оценивается по полному принятому результату, времени и переделкам, а не количеству агентов или вызовов.

**Non-Goals:** перенос всего `CODEX_HOME` в репозиторий, общий memory daemon, векторная база, генерация памяти фоновыми моделями, fork/patch самого CLI, новый оркестратор, обязательные worktree и JSON-run на любую задачу. Исключения пользователя — в proposal. Инфраструктурные исправления соседних changes не входят в эту спеку.

## Decisions

### 1. Git-память: индекс, существующие документы и один глобальный skill

Поставка — `.agents/skills/project-memory/SKILL.md` с коротким шаблоном. Данные по умолчанию — `docs/memory/README.md` и небольшие тематические Markdown-файлы только для знаний, у которых ещё нет владельца. Проектный `AGENTS.md` содержит краткий маршрут к индексу и указывает читать подходящие темы в начале существенной работы и после потери контекста. При внедрении в существующий проект используем его уже объявленный путь; не создаём конкурирующий стандарт автоматически.

Пример будущего индекса harness:

```text
AGENTS.md
  → docs/memory/README.md
      → ../project-decisions.md       подтверждённые решения
      → ../installation.md            подключение и откат
      → ../source-diagnostics.md      актуальные точки диагностики
      → подходящие evidence/записи    проверенные ограничения и случаи
```

Это навигация, а не копия всех документов. Индекс содержит тему, краткое назначение и ссылку. Запись знания содержит утверждение, область применимости, статус (`confirmed`, `verified`, `tentative` или `superseded`), основание и дату/ревизию проверки. Поля допустимы обычным текстом; отдельная база метаданных не нужна. Для пользовательского решения не изобретаем техническую source revision; достаточно подтверждения и даты. Для команды указываем cwd, существенные параметры и ссылку на текущий источник, переиспользуя формат `project-verification`.

Чтение: установить текущий корень → прочитать маршрут/короткий индекс → открыть подходящие записи → проверить технические сведения, если их область изменилась. Запись: подтверждённое решение сохраняется в ходе обсуждения, воспроизведённая полезная находка — после проверки; обычный прогресс и повторённые факты не порождают запись. Обновление выполняется поверх свежего содержимого, в текущем worktree; никакого автоматического commit/push или изменения соседних checkout. Модель не делает отдельный вызов ради memory maintenance.

Альтернатива native memories не соответствует выбранному владельцу данных: [документация](https://learn.chatgpt.com/docs/customization/memories) описывает generated state под Codex home. Перенос всей home затрагивает auth, историю и runtime. Зависимость от `.serena/memories` также не нужна для базового сценария: checked-in текст и AGENTS-маршрут работают без Serena. Уже существующие проектные записи Serena можно прочитать и сослаться на них после проверки владения, но новый сервер или синхронизация двух хранилищ не вводятся.

Приёмочный consumer: небольшой независимый Git-репозиторий с известным решением, релевантной технической записью, нерелевантной темой и устаревшей командой. В новом Codex home с установленным kit проверить выборочное чтение, использование решения, исправление устаревшей команды и запись одного нового проверенного факта. В owned fixture commit разрешён для создания проверяемого clone; после clone новый агент использует эти записи без прежней истории. Отдельно проверить Git-конфликт двух версий записи и сохранение файлов после отключения kit.

### 2. Экспериментальный контекст — обратимый пилот в существующем профиле

Настройка по [документации моделей](https://learn.chatgpt.com/docs/models#experimental-context-management):

```toml
[features.context_management]
experimental_mode = true
```

Начать с one-shot `-c 'features.context_management.experimental_mode=true'` на новом Astra task, затем при подтверждённой доступности подключить выбранное значение в `global/harness.config.toml`. Не смешивать boolean и table на одном TOML-пути. Добавить проверку нужного managed setting в существующие manifest/diagnostics только если без неё жизненный цикл не покрыт. Проверять новый процесс: текущая сессия автоматически не приобретает новый механизм.

Accepted Astra runtime pilot 2.1 already distinguished parser, effective setting, eligibility and sampling `ContextManagement`. Linked source flag is enabled; separate ordinary runtime, false override and profile rollback passed task 2.2 with actual sampling records. Native memories and Fast stay excluded. [Delivery evidence](../../../../docs/evidence/native-context-delivery.md) retains the earlier model-free limitations and setup failures.

Страница моделей указывает opt-in для поддерживаемых клиентов с ChatGPT Plus/Pro и отсутствие Business/Enterprise/API-key sign-in на старте. Схема подтверждает допустимость настройки, но не тариф или право конкретного аккаунта. Фиксировать фактическую subscription route и признак effective activation, доступный текущему runtime. Если такой признак нельзя установить, не выдавать принятие TOML за успешный пилот.

Проверка поведения: ограниченная задача с ранним конкретным ограничением и дальнейшим продолжением, где это ограничение влияет на независимый ожидаемый ответ. Не заполнять контекст бессмысленными токенами ради теста; если переход между окнами не произошёл, явно ограничить вывод проверкой активации и обычного продолжения. Эффект при естественной длительной работе записать отдельно, без обещания заранее измеренной экономии. Откат — вернуть прежнее значение/удалить только добавленную настройку и запустить новую задачу.

### 3. `/btw`: доказанная доступность и диагностика состояния

Native executable установленного npm package: `@openai/codex-win32-x64/.../bin/codex.exe`, **0.153.4**, SHA-256 `444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`. В качестве harness использован существующий `tests/ConPty.cs`; отдельные временные Codex home/workspace, read-only sandbox, Astra low, локально отключённые hooks. Live конфигурация, активный диалог и shared proxy не изменялись.

Результаты исследования 2026-09-08:

| Попытка | Триггер | Результат |
| --- | --- | --- |
| Пустой диалог, v2=false | `/btw`, затем `/side` | Обе команды распознаны; обе требуют сначала начать основной разговор; `/quit` завершился естественно с кодом 0 |
| Пустой диалог, v2=true | Те же команды и настройки, изменён только v2 | Тот же результат, естественный exit 0; гипотеза о необходимости v2 для регистрации не подтверждена |
| Начатый диалог, v2=false | Один короткий запрос Astra, дождаться `task_complete`, затем `/btw` | Получен `READY_SIDE`, открыт режим `Side from main thread`; дополнительных запросов модели в side не отправлялось |
| Уже открыт side | `/side`, затем `/quit` | Обе команды отвергнуты как недоступные внутри side; UI требует Ctrl+C для возврата. Попытка `/quit` не дала natural exit, fixture завершён своим process handle; это ошибка выбранной последовательности очистки, не успешный тест выхода |
| Resume собственного fixture, v2=false | `/btw`, один Ctrl+C во время загрузки side MCP, затем `/quit` | Открытие side подтверждено; возврат и natural exit этой последовательностью не подтверждены, fixture завершён своим process handle. Это ограничение автоматической проверки клавиш/готовности, без установленной причины |

Точное сообщение в пустом диалоге: `'/side' is unavailable until the current conversation has started. Send a message first, then try /side again.` Имя `/side` в ошибке при вводе `/btw` ожидаемо для alias, само по себе оно не означает опечатку. В side сообщение указывает: `Press Ctrl+C to return to the main thread first.` [Официальный контракт](https://learn.chatgpt.com/docs/developer-commands?surface=cli#start-a-side-chat-with-side) дополнительно исключает review mode; этот режим в исследовании не упражнялся.

Приватные исходные receipts и сценарии оставлены в temp-каталоге `codex-btw-research-be4242a71c844d2eaef5eac727d603c0`: `probe.ps1`, `result.json`, `started-probe.ps1`, `started-result.json`, `return-probe.ps1`, `return-result.json`. Их содержимое не нужно загружать в Git; для воспроизведения достаточно описанной последовательности и fixture helper. Всего отправлен один короткий Astra model request; остальные действия модели не вызывали.

**2026-09-08 пользователь подтвердил: `/btw` заработал.** Разбор его сбоя закрыт; конкретная прежняя причина не установлена и не приписывается экспериментальным настройкам. Включать `multi_agent_v2` ради команды оснований нет. В документации оставить базовый маршрут: начатый основной разговор → `/btw` или `/side` → боковой вопрос → рекомендованный самим UI Ctrl+C для закрытия side. Автоматический fixture подтвердил открытие и ограничения, но не полный возврат; подтверждение пользователя записано отдельно. Повторный поиск причины и дополнительные model calls ради этого закрытого эпизода не входят в implementation tasks.

Дополнительный fallback для side не создаём: текущая native возможность работает у пользователя. Обычный `/fork` разделяет историю и сохраняет общий filesystem; для параллельных изменений требуется worktree.

### 4. Worktrees через Git и подготовку целевого проекта

Поставка — один глобальный skill `.agents/skills/isolated-worktree/` с короткой процедурой. Применять для независимых записей и рискованных экспериментов, когда ожидаемая выгода покрывает подготовку и интеграцию. Для короткой тесно связанной правки работать в текущем checkout.

Основной путь: `git worktree add -b <owned-branch> <owned-path> <explicit-base>`, затем native setup и проверки из `project-verification`. Пути/ветки генерируются с проверкой коллизий. Если задача зависит от dirty inputs, передать только согласованный scoped snapshot (включая необходимые untracked files), проверить его соответствие, либо оставить связанную работу в родителе. Не использовать автоматический stash/reset или commit пользовательских изменений. Fixture-коммиты принадлежат только тестовому репозиторию.

Каждый child получает фактический worktree root, исходную ревизию и отличия, владение файлами, нужные skills, инварианты и проверки. Git metadata может быть файлом `.git`; не распознавать проект только по наличию каталога. Serena/LSP/Codebase Memory выбирают этот checkout; неизвестный или изменившийся индекс обновляется до query, отсутствие подходящего покрытия явно переводит работу на свежий scoped source. Глобальные config/service/auth, порты и внешние БД worktree не изолирует.

После результата проверить diff, интегрировать только принадлежащие задаче изменения, разрешить конфликты и повторить применимые проверки на итоговом дереве. `git worktree remove` выполняется после подтверждения сохранности работ, без `--force`; ветка не удаляется автоматически при неизвестном состоянии. Отдельная инфраструктура для app Handoff и desktop setup не нужна: [документированный app workflow](https://learn.chatgpt.com/docs/environments/git-worktrees) не является CLI API.

### 5. Structured exec как ограниченный consumer, без нового runtime

Поставка — `.agents/skills/structured-codex-run/` с примером JSON Schema, процедурой и минимальной детерминированной проверкой результата. Native форма:

```powershell
codex exec --model gpt-6-astra --sandbox read-only --ephemeral `
  --output-schema <schema.json> --json --output-last-message <unique-result.json> -
```

Prompt подаётся через stdin; аргументы передаются отдельными значениями, без интерполяции пользовательского текста в shell. Для реальных разрешённых изменений sandbox выбирается из задачи явно: глобальный harness использует Full Access, и ссылаться на read-only default из общей документации недостаточно. [Native help и документация](https://learn.chatgpt.com/docs/non-interactive-mode) различают event JSONL и schema-constrained final response.

Первый реальный consumer — read-only проверка небольшого fixture-репозитория: итог содержит findings, пути/evidence и unresolved issues; независимый oracle заранее знает заложенную проблему. Schema использует поддерживаемое подмножество native structured output; верхний объект и обязательные поля проверяются независимо. Процессный receipt с run identity, natural exit и timeout принадлежит наблюдателю, а не модели. `status: success` в JSON не является доказательством.

Переиспользовать имеющееся наблюдение процессов (`reproduce-regression` / Windows supervisor) и проектные проверки. Развивающиеся `tools/outcome_runner.py` / `tools/outcome_report.py` не превращать в новый универсальный Codex orchestration API: их приёмочный формат и JSON результата модели имеют разные обязанности. Добавлять helper только для конкретного отсутствующего звена: уникальный output, валидация schema, сведение receipt и независимого результата проверки.

Grok остаётся preferred middle для подходящего делегирования. Наличие parseable JSON у external route не доказывает поддержку native `--output-schema`; не отправлять Grok через неподтверждённый контракт и не заменять provider молча. Native приёмочный вызов явно использует Astra и существующую ChatGPT subscription. Отказы, timeout, output limit, malformed/missing JSON, stale output и schema-valid wrong answer проверяются преимущественно детерминированными fixtures.

## Risks / Trade-offs

- Устаревшая память подменяет актуальные условия → короткие записи с областью и основанием, проверка изменившихся источников и приоритет текущих инструкций.
- Индекс превращается в ещё один большой prompt → индекс содержит маршруты, тела читаются по задаче; лишняя запись не обязательна.
- Bounded Astra runtime pilot 2.1 и ordinary runtime/false override/profile rollback 2.2 приняты по отдельным sampling records. Общая lifecycle-приёмка навыков пройдена отдельно; parser/flag/model-free startup сами по себе её не заменяют.
- Worktree тест затрагивает live proxy → собственные ресурсы и запрет считать filesystem-изоляцию изоляцией глобальных сервисов.
- Валидный JSON скрывает неверный итог/timeout → natural termination, schema и независимый oracle проверяются отдельно; partial evidence сохраняется.
- Работа в параллельных сессиях меняет исходники и документы → точечные изменения поверх свежего состояния, без сброса чужого diff; stale граф/диагностика не считаются проверкой.

## Migration Plan

1. Создать переносимые skills и примеры, используя `skill-creator`, добавить короткие проектные маршруты и связать существующие записи harness. Новых обязательных сервисов нет.
2. Подключить через существующий install lifecycle, проверить обнаружение из независимого проекта; поправить managed registration только при необходимости. Источники должны оставаться прямыми, без ручных копий.
3. Bounded Astra runtime pilot 2.1, ordinary runtime/false override/profile rollback 2.2 и `/btw` confirmation already exist. Global skill lifecycle also passed, without restoring ordinary hooks, Fast or native memories; final results are in docs/evidence/native-workflows.md.
4. Проверить Git-memory clone, worktree интеграцию и structured-exec consumer; прогнать соответствующие отказные и lifecycle случаи. Отдельно записать фактическое время и rework; не обещать процент экономии по одному примеру.
5. Обновить documentation/evidence и закрыть implementation tasks только по результатам. Отключение удаляет свои global registrations и возвращает своё изменённое значение контекста; проектную память и полезные результаты не удаляет. Не останавливать shared services текущих сессий.
