Состояние: выполнены все 45 задач, глобальная активация оставлена рабочей; change архивирован 2026-09-06 после синхронизации основных спек. Языковой объём: Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake и Bash. Из условных языков HTML/CSS подключены и проверены, YAML/QML недоступны. [Итоговый отчёт](../../../../docs/code-tools-verification.md) сопоставляет задачи с фактическими проверками и ограничениями среды.

Нормативный объём: [dependencies](specs/tool-dependency-lifecycle/spec.md), [MCP и каталог языков](specs/global-code-tools/spec.md), [автодиагностика](specs/automatic-lsp-diagnostics/spec.md), [глобальный lifecycle](specs/linked-global-kit/spec.md). Архитектура и источники — в [design.md](design.md).

## 1. Первый сквозной consumer на существующих установках

- [x] 1.1 Подтвердить реальной изолированной CLI и unprofiled app-server/desktop проверкой загрузку MCP через нативную регистрацию-путь и hooks через ссылку на checkout; изменение исходника должно читаться новым consumer без deployment-копии.
- [x] 1.2 Доказать PostToolUse → native mcp_tool hook → model-visible additionalContext на установленном Codex; проверить JSON-поля и MCP response envelope, native trust, отсутствие рекурсии и сохранение исходного результата tool.
- [x] 1.3 Получить сквозную автодиагностику существующего TS backend после реального patch и исправления ошибки; сохранить trace без ручного diagnostic call и без обновления общей установки.
- [x] 1.4 Проверить обнаружение фактических shell/MCP changes и свежести LSP на минимальных примерах pre-dirty/untracked, partial failure, yielded command и быстро исправленной ошибки; зафиксировать рабочие контракты, а не оставлять предположения в design.
- [x] 1.5 Доказать сохранение первой правки при намеренно задержанном handshake adapter и немедленном completion, а также доставку pending/error/clearance до возврата подагента; проверить независимый baseline, Stop/SubagentStop и child identity.

## 2. Общие зависимости и установка

- [x] 2.1 Расширить repository inventory декларациями всех MCP/LSP, источников, runtime и version policy; сверить машиночитаемую языковую матрицу с нормативным каталогом, без пропущенных строк.
- [x] 2.2 Реализовать discovery существующих uv/npm/rustup/LSP установок с ownership и фактическими версиями; проверить reuse, локальную модификацию, отсутствующий executable и конфликт двух Graphify-команд.
- [x] 2.3 Добавить явный install/update план и non-mutating preview с официальными metadata; проверить отсутствие изменений при preview, сетевую ошибку, отсутствие нового релиза и отсутствие update во время сессии/diagnostics.
- [x] 2.4 Реализовать установку отсутствующих distributable dependencies и подключение существующих SDK через lifecycle; проверить в чистом окружении, что известные зависимости переиспользуются, а missing licensed SDK получает точную причину.
- [x] 2.5 Реализовать проверку совместимости обновления, сохранение восстановления, учёт активных потребителей и блокировку конкурентной замены; воспроизвести несовместимый релиз и успешный rollback без повреждения конфигурации/данных OpenCode.
- [x] 2.6 Расширить Check/Recover/Disconnect и состояния регистрации для внешних tools; проверить повторный запуск, partial activation, stale ownership, сохранность adopted packages/cache и отсутствие удаления через ссылки.

## 3. Подключение четырёх MCP

- [x] 3.1 Подключить существующую Serena с контекстом Codex и правильным workspace; проверить реальные symbol/reference/read и bounded semantic-edit/diagnostic calls в двух проектах.
- [x] 3.2 Подключить Codebase Memory с существующей общей установкой и корректным project identity; проверить реальное индексирование disposable проекта, code query и отсутствие результатов соседнего проекта.
- [x] 3.3 Подключить Graphify через проверенный graphifyy, сохранённый graph и локальную авторизацию, с reuse здорового HTTP и stdio fallback; проверить оба маршрута, graph query, явный repo и отказ repository operation без repo.
- [x] 3.4 Подключить Nuphus из существующего npm package и сохранить применимые настройки; проверить реальное desktop/browser чтение и ограниченное взаимодействие с disposable target, не затрагивая рабочие приложения.

## 4. Автоматическая диагностика

- [x] 4.1 Реализовать узкий harness-lsp adapter с ленивыми LSP clients, существующими binaries/cache и workspace identity; проверить повторное использование процесса, штатное завершение и сохранность другого consumer.
- [x] 4.2 Реализовать независимый pre-edit bootstrap/baseline, dirty journal и content reconciliation для create/change/rename/delete, roots и project config; проверить первую правку до готовности MCP, pre-dirty/untracked, ignored-but-edited source, watcher overflow и отсутствие full analysis при read-only tool.
- [x] 4.3 Реализовать синхронную diagnostics_after_tool операцию и глобальный PostToolUse; реальными patch/shell/MCP edits подтвердить автоматическую model-visible доставку, multi-file edits и nonzero/yielded shell.
- [x] 4.4 Реализовать push/pull freshness, current empty clearance, bounded/truncated summaries и dependent diagnostics; проверить stale response после нового edit, ошибку импортирующего файла и изменение compiler options.
- [x] 4.5 Реализовать finite timeout, startup-not-ready, pending reconciliation через Stop/SubagentStop с command fallback при отсутствующем MCP и cleanup; проверить child identity, отсутствующий/crashed backend, позднюю background write, отсутствие infinite loop/autofix и сохранение исходного tool result.

## 5. Выбранные пользователем языки

- [x] 5.1 Для TypeScript и JavaScript проверить выбранную совместимую пару language server/runtime, project-local TS settings, навигацию и автоматическое введение/снятие ошибки в реальном Codex; обновить общую установку при совместимом новом релизе.
- [x] 5.2 Для Rust переиспользовать rustup Rust Analyzer и проверить definitions/references, project config и автоматическое введение/снятие ошибки; не обновлять проектный toolchain как побочный эффект.
- [x] 5.3 Для PowerShell проверить совместимый PSES update, reuse PSScriptAnalyzer, symbol/reference operations и автоматическое введение/снятие ошибки в реальном Codex.
- [x] 5.4 Для Delphi проверить существующий Pascal backend на представительном Delphi проекте: диалект/SDK, несколько units/include paths, definitions/references и автоматическое введение/снятие ошибки; при недостаточности подключить совместимый backend без переписывания проекта.

- [x] 5.5 Для Python выбрать и переиспользовать совместимый backend, проверить project environment/import resolution, definitions/references и автоматическое введение/снятие ошибки в реальном Codex; обновить общую установку при совместимом новом релизе.

- [x] 5.6 Для C++ переиспользовать существующий сервер, проверить include/build configuration, definitions/references и автоматическое введение/снятие ошибки в реальном Codex.
- [x] 5.7 Для C# обнаружить и переиспользовать существующий сервер/SDK, проверить project resolution, definitions/references и автоматическое введение/снятие ошибки в реальном Codex.
- [x] 5.8 Для JSON подключить готовый совместимый backend и проверить применимые операции, project schemas и автоматическое введение/снятие ошибки в реальном Codex.
- [x] 5.9 Для Markdown подключить готовый совместимый backend и проверить фактические navigation/diagnostic возможности с автоматическим введением/снятием поддерживаемой ошибки в реальном Codex.
- [x] 5.10 Проверить наличие готовой совместимой поддержки YAML: при наличии подключить и проверить применимые операции и автодиагностику; при отсутствии явно записать unavailable без обязательной установки нового backend.

- [x] 5.11 Для TOML подключить и проверить backend, применимые project schemas и автоматическое введение/снятие ошибки.
- [x] 5.12 Для XML подключить и проверить backend/schema resolution, применимые операции и автодиагностику; проверить отличие Qt XML .ts от TypeScript.
- [x] 5.13 Для CMake подключить и проверить CMakeLists.txt/модули, project context, применимые операции и автодиагностику.
- [x] 5.14 Для Bash подключить и проверить применимые операции и автодиагностику без исполнения анализируемого скрипта.
- [x] 5.15 Проверить готовую совместимую поддержку QML, HTML и CSS: доступные backends подключить и проверить; отсутствие явно записать без обязательной установки новых backends.

## 6. Проверка системы и исправление дефектов

- [x] 6.1 Проверить все MCP и LSP через обычные Codex-сессии вне harness, включая TUI, exec, resume/fork, tool-capable subagents и установленные применимые desktop/IDE consumers; подтвердить отсутствие project-local registration и сохранение explicit overrides.
- [x] 6.2 Проверить одновременные проекты с одинаковыми символами, worktrees, subagents, spaces/non-ASCII paths и additional roots; подтвердить правильные project/generation results и независимое завершение процессов.
- [x] 6.3 Проверить reuse/install/update/rollback/conflict/reconnect/disconnect и сохранность credentials, indexes, graph, чужих MCP/hooks и project config; после общих обновлений выполнить meaningful проверки существующего OpenCode.
- [x] 6.4 Выполнить применимые исходные launcher/installer/consumer/global-activation проверки и Windows TUI/trust проверки при изменении их контрактов; устранить обнаруженные регрессии, не подменяя реальные проверки mocks.
- [x] 6.5 Провести независимое ревью конкретных P0/P1 путей: shared update/data recovery, workspace isolation, stale diagnostics, shell input и direct-source lifecycle; исправить существенные дефекты и повторить затронутые сценарии.

## 7. Глобальная поставка и итоговая приёмка

- [x] 7.1 Выполнить preview и проверенные совместимые обновления на текущем ПК; сохранить версии, provenance, rollback и успешную совместную проверку Codex/OpenCode, явно отразив обоснованно удержанные версии.
- [x] 7.2 Активировать все MCP и автодиагностику глобально, завершить необходимые нативные trust действия и доказать реальные вызовы из новых сессий в neutral directory и двух проектах вне harness; оставить рабочую глобальную активацию.
- [x] 7.3 Пройти установку из свежего checkout в чистом Windows окружении без неописанных файлов старого ПК; проверить missing dependencies, перенос checkout и повторный запуск, описав ограничения заменителя второго физического ПК.
- [x] 7.4 Обновить installation, карту возможностей и постоянные решения; выпустить acceptance report с соответствием всех требований/сценариев и строк матрицы фактическим результатам, проверить локальные ссылки и актуальность источников.
- [x] 7.5 Сверить всю спеку с результатами, убрать test-only registrations и безопасно завершить проверочные процессы; закрыть задачи только по доказательствам и выполнить строгую OpenSpec-валидацию, сохранив незакрытое состояние при любом оставшемся обязательстве.
