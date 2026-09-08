# Расширения исходников в mekha

Исторический инвентарь, не текущая обязанность установки LSP. Состав после
оценки расхода и границы проверенной поддержки — в [актуальной матрице](code-tools.md#current-language-selection).

Проверка 2026-09-06: read-only обход `\\PC\mekha` через `rg --files --hidden --no-ignore`; содержимое файлов не читалось. Обход завершился с exit 0, ошибок доступа не было. После исключения основных каталогов VCS, зависимостей и сборки перечислено 297 202 файла и 495 расширений. `--follow` не использовался; доступный каталог SynologyDrive вошёл в результат. Числа ниже включают копии, fixtures и оставшиеся артефакты и не являются числом уникальных собственных исходников.

| Наблюдение | Где обнаружено | Вывод для выбранной поставки |
| --- | --- | --- |
| Rust: 621 `.rs`; 77 `.toml` | controller-gateway-service, hmi-rs, hmi-2_0, pmac-emulator | Rust и TOML |
| C++: 865 `.cpp`, 57 `.cc`, 1047 `.h`, 81 `.hpp` | mcsetup, constructor, ladder, PcommServer и другие | C++ с проектными include/build inputs |
| `CMakeLists.txt`, 23 `.cmake` | Собственные CMakeLists в mcsetup/constructor; часть модулей в External | CMake; `.txt` не означает только обычный текст |
| Delphi: 399 `.pas`, 93 `.dpr`, 90 `.dproj` | hmi-legacy и его копии | Delphi; копии не считать тремя независимыми проектами |
| 188 `.ps1`, 1 `.psm1`, 1 `.psd1` | controller-gateway-service, pmac-emulator, windows-ui-automation | PowerShell |
| 66 `.py` | В основном mcsetup/External; собственные scripts в mnc-studio, hmi-legacy, graphify-knowledgebase | Python; не считать всю внешнюю библиотеку собственным кодом |
| 65 `.ts`, 1 `.js` | gitflic-cli, Qt-каталоги mcsetup/constructor, plugin | TypeScript/JavaScript; Qt `.ts` может быть XML-переводом — требуется проверка формата при выборе LSP |
| 5 `.cs`, 3 `.csproj` | windows-ui-automation | C# |
| 33 207 `.json`, 2631 `.md` | Много проектов; большая доля JSON в данных/отчётах | JSON и Markdown, без анализа всех результатов сборки |
| 3757 `.xml`, 41 `.ui`, 5 `.qrc` | parameterizer, mcsetup, controller, HMI | XML; Qt XML dialects не равны QML |
| 17 `.sh` | hmi-rs/scripts и .claude/hooks; два файла в External | Bash |
| 84 `.yaml`, 11 `.yml` | OpenSpec configs; часть YML во внешних зависимостях | YAML при наличии готового совместимого backend |
| 11 `.qml` | SynologyDrive/Тестирование/HMI/HMI 2_0 | QML условно; совместимость с используемым Qt ещё не проверена |
| 11 190 `.html`, 7 `.htm`, 4 `.css`, 2 `.qss` | Главным образом отчёты; собственная справка ladder и Qt styles | HTML/CSS условно; QSS не объявлен стандартным CSS без проверки |
| 16 `.kdl` | hmi-rs/tests/fixtures/kdl | Кандидат для отдельного исследования, не принятый LSP |
| `.pmc`, `.plc`, `.mnc`, `.nc`, `.g`, `.56k` | controller, pmac-emulator, HMI и сохранённые комплекты | Специальные PMAC/ЧПУ-форматы; расширение не доказывает совместимость с произвольным PLC/G-code сервером |

Подтверждённый состав — в [постоянных решениях](project-decisions.md#глобальные-mcp-lsp-и-автоматическая-диагностика) и [нормативной спеке](../openspec/specs/global-code-tools/spec.md). Кандидаты проверяются по официальным источникам: [Taplo/TOML](https://taplo.tamasfe.dev/), [LemMinX/XML](https://github.com/eclipse-lemminx/lemminx), [neocmakelsp/CMake](https://github.com/neocmakelsp/neocmakelsp), [Qt QML Language Server](https://doc.qt.io/qt-6/qtqml-tooling-qmlls.html). Наличие проекта сервера не означает установленную или проверенную интеграцию на этой машине.
