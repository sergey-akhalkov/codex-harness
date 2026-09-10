# Диагностика источников Codex Harness

Из любого каталога в PowerShell:

```powershell
codex-harness-check.ps1 -Json
codex-harness-check.ps1 -ProjectPath D:\path\to\project -Json
```

Из checkout эквивалентная команда: `./install.ps1 -Mode Check -Diagnose -ProjectPath <directory>`. Без `-Json` глобальная команда возвращает PowerShell-объект. Статус проверяется по полю `status`, а не коду завершения процесса. Обычный `Check` сохраняет проверки установки и протоколов. Диагностика не исправляет найденные конфликты.

## Что возвращается

В native-кандидате доступна `codex-harness.exe diagnose` (также `check --diagnose`) с параметрами
`--project`, `--source`, `--codex-home`, `--user-home`,
`--dependency-user-home`, `--upstream`, `--profile` и `--timeout-seconds`.
Его core-установка регистрирует `codex-harness-check.exe`, который вызывает ту же
диагностику. Пути CLI разрешаются относительно каталога вызывающей команды;
отчёт всегда JSON. [Проверки native-порта](evidence/rust-source-diagnostics.md)
отделены от ещё не выполненного глобального переключения: текущая глобальная
команда `.ps1` пока остаётся рабочим входом.

| Поле | Значение |
| --- | --- |
| `status` | `healthy`: проверка завершилась без находок; `attention`: есть конфликт или намеренное переопределение; `incomplete`: часть обязательных наблюдений недоступна |
| `links` | Назначение, ожидаемый и фактический источник каждой управляемой ссылки |
| `layers` | Пути конфигурационных слоёв, профиль, active/disabled; порядок от низшего приоритета к высшему |
| `settings` | Ограниченный набор деклараций, вычисленный победитель, происхождение и признак переопределения |
| `skills` | Имя, путь, scope и enabled из native `skills/list`; совпадения имён разных файлов отмечаются отдельно |
| `findings` | Категория, затронутый источник и действие для восстановления |
| `freshness` | Состояние уже запущенных сессий и MCP/LSP остаётся `unknown` |

Проверяются `model`, `model_reasoning_effort`, `approval_policy`, `sandbox_mode`, `developer_instructions`, `features.hooks`, `features.multi_agent`, `features.memories`. Выводятся только разрешённые значения предпочтений; прочие значения скрыты. Для developer instructions доступны наличие и источник, без текста. Отсутствующая декларация не заменяется догадкой о default. Намеренное проектное переопределение тоже требует внимания, но не считается ошибкой конфигурации.

## Граница доказательств

Проверен CLI **0.153.4**. Его app-server принимает `cwd` и `includeLayers` в `config/read`, но не принимает `--profile harness` или legacy `-c profile=...`. Поэтому диагностика читает базовые слои native-потребителем, отдельно разбирает файл профиля самим Codex через временную ссылку и вычисляет победителей выбранных простых настроек. В отчёте это явно обозначено `inferred-from-native-layers`. Это не полный экспорт effective configuration выбранной CLI-сессии. Навигация и trust базового потребителя берутся из native-слоёв.

Если профиль меняет относящийся к проекту trust, discovery или настройки skills, есть managed requirements либо версия CLI ещё не проверена, результат `incomplete`: неопределённые effective значения не утверждаются. Skills наблюдаются базовым потребителем. Уже работающие процессы, их загруженный код, runtime defaults и произвольные CLI overrides другого запуска не проверяются. После изменения источников нужен новый соответствующий потребитель; исправная ссылка не доказывает обновление уже работающего сервера.

Контракты: [официальная иерархия конфигурации](https://learn.chatgpt.com/docs/config-file/config-basic), [CLI 0.153.4](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/cli/src/main.rs), [загрузчик слоёв](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/config/src/loader/mod.rs). Схемы RPC дополнительно проверены генератором установленного `codex app-server generate-json-schema --experimental`.

## Эффекты и восстановление

Нет model requests, запуска пользовательских project commands/hooks, thread/turn calls или управления существующими сервисами. Native чтение может создать собственные runtime caches/logs; временный home профиля удаляется после остановки его процесса. Общий срок RPC — 30 секунд, затем ограниченная очистка собственных процессов. Raw configs, prompt bodies, skill descriptions, stderr и native parser errors в отчёт не попадают. Пути и имена источников являются диагностическими данными; перед внешней публикацией отчёта проверяйте их пригодность к раскрытию.

Команда входит в обычный inventory Install/Update/Disconnect/Recover. Не заменяйте повреждённую чужую ссылку автоматически: сначала установите её владельца. Для полного обновления набора используется обычный `install.ps1 -Mode Update`. При обновлении только inventory действующей полной установки оператор может вызвать `Invoke-HarnessInstall -IncludeCodeTools`, сохранив параметры зарегистрированного home и dependency owner; это не управляет сервисом подписок. `-CoreOnly` создаёт только core inventory и не предназначен для сохранения дополнительных hook links полной установки.

Сценарии и наблюдения приёмки: [evidence](evidence/source-diagnostics.md). Выбор первого пакета и исправления исходных рекомендаций: [аудит](opencode-kit-audit.md).
