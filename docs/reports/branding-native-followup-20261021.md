# Исполнение native regression: 21 октября 2026

Hosted run `39537468174` на head `7661af68` упал в тесте scratch cleanup с
`ExecutableFileBusy / Text file busy`. Это code/test failure, не billing.

Linux-фикстура исполняет уже загруженный immutable test binary через
`/proc/self/exe`, вместо записи новой копии рядом с параллельными fork.
Реальный drop uid и filesystem denial сохранены; на остальных Unix сохранена
старая staged-copy реализация.

При параллельном воспроизведении дополнительно обнаружено предположение об
немедленном освобождении capacity после drop: чужой fork может кратко удерживать
CLOEXEC до exec. Тест ограниченно ждёт завершения этой kernel custody; ошибки
блокировки по-прежнему падают. Отдельные проверки удержания lock/capacity orphan
процессом до его фактического завершения не изменены и проходят.

Rust 1.99 clippy `-D warnings` PASS. Native infra suite выполнен 12 раз с восемью
test threads: каждый раз 11 PASS, четыре opt-in cases явно ignored. Это 12
повторов одного набора, а не 132 различных сценария. Production behavior не
менялся, кроме эквивалентной записи существующего fast-forward guard для clippy.
Минимальный Rust учитывается отдельным прогоном, не выводится из версии 1.99.

Frontend tests/lint/build, frozen install и входящий Vite 6.4.4 security patch
проверены отдельно. Последующий native/test-only diff не меняет frontend inputs.
Установленные Git repositories, runner, Docker runtime и credentials не менялись.
