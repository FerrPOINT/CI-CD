# Установка CLI CI/CD

Локальный candidate подготовлен 2026-10-02 из main `bd728e9e7dd57c14331d1341bb9a74614ac5bfc4`, Base `c083783a37791e277db796361203884b87828a7d`, Rust 1.88.0, `cargo build --locked --release -p cicd-cli`. Package version: `0.1.0`; версию продукта не меняли, Git tags и публичный release не создавали.

## Артефакт и требования

Архив: `cicd-cli-0.1.0-bd728e9-x86_64-linux-gnu.tar.gz`. SHA-256 архива: `ff6e9cccf5343d88644b26f82804e4bfc39ca3733cbc00a2bc200b8fc5493188`. Внутри только `cicd-cli`, `README.md` и `SHA256SUMS`; credentials и данные стенда не включены.

GNU/Linux x86_64, glibc >= 2.34. Живой API-прогон выполнен в Ubuntu 24.04 WSL; запуск/справка дополнительно проверены в Debian 12. Native Windows/macOS и musl-сборки не подготовлены. На Windows используйте WSL. У CLI пока нет `--version`; source/package version и checksum берутся из manifest.

## Установка в Linux / WSL

После получения архива сверьте его SHA-256 с manifest, затем:

```bash
workdir=$(mktemp -d)
tar -xzf cicd-cli-0.1.0-bd728e9-x86_64-linux-gnu.tar.gz -C "$workdir"
(cd "$workdir" && sha256sum -c SHA256SUMS)
mkdir -p "$HOME/.local/bin"
install -m 755 "$workdir/cicd-cli" "$HOME/.local/bin/cicd-cli"
export PATH="$HOME/.local/bin:$PATH"
cicd-cli --help
```

При существующей установке сначала сохраните предыдущий binary для отката. Для проверки этой поставки использован отдельный prefix `/root/.local/share/sdlc-cli/cli-20261002/bin`; прежние команды не перезаписывались.

## Подключение к sdlc1

```bash
export CICD_API_URL=http://127.0.0.1:7711
cicd-cli project list --limit 1
```

Передайте PAT через `SDLC_API_TOKEN` либо продуктовую переменную из [CLI.md](CLI.md). Read-команды требуют `ci-cd:read`, mutations — соответствующий `ci-cd:write`; scopes не отменяют серверную авторизацию. Значение token не помещайте в аргументы, историю shell, manifest или release notes. URL здесь относится к sdlc1; для другого стенда задайте его явно.

## Статус приёмки и откат

Это candidate: полная приёмка против принятого runtime не завершена. Ограничения и результаты — [CLI_VALIDATION.md](CLI_VALIDATION.md). Обновление CLI не обновляет backend; рабочие runtime images/pins в этой проверке не заменялись. После согласованного обновления backend повторить блокирующие сценарии; затем решать о tags, версиях и публикации release. Для отката верните предыдущий binary и конфигурацию URL.

## References

- [CLI](CLI.md)
- [CLI validation](CLI_VALIDATION.md)
- [Base integration](BASE_INTEGRATION.md)
