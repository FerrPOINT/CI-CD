# Установка CLI CI/CD

Принятая локальная поставка Linux x86_64/WSL от 2026-10-03. Backend sdlc1 обновлён;
live acceptance и 15 минут наблюдения пройдены. [Матрица и откат](CLI_VALIDATION.md).

## Артефакт и требования

Архив `cicd-cli-0.1.0-32e33aa-x86_64-linux-gnu.tar.gz`; SHA-256:
`1ba34fbcfe708bc95ece8e4022ee24cd3e5e4a60dec1b77311dbd6d72b1bf057`.
Binary SHA-256: `8c4d059be874ec771e7d111a88edb8efb0a5948b89566ae818a469a50f433dc5`.
Code source `32e33aaf268df14111ef9f1df1b0c4846aa7c6b2`; Base `9408802dfa978cba2f67162a49adca6f65851b01`.
Package `cicd-cli` version `0.1.0`, Rust 1.88.0, locked release build.
Версии/tags/public release не менялись. В архиве только binary, README.md,
SHA256SUMS; credentials/configuration/keys/data отсутствуют.

GNU/Linux x86_64, glibc >= 2.34; OpenSSL 3
этому binary не требуется.
Ubuntu 24.04 WSL и Debian 12: outer/inner checksum, отдельный installation prefix,
help, runtime libraries и authenticated read всех трёх API sdlc1 — PASS.
Windows используйте через WSL; native Windows/macOS/musl не поставляются.

## Установка

Получите архив и общий SHA256SUMS из локального комплекта; сначала сверьте outer
checksum, затем установите в новый prefix. Пример не перезаписывает предыдущую
установку и не меняет shell profiles:

```bash
sha256sum -c SHA256SUMS
stage=$(mktemp -d)
tar -xzf cicd-cli-0.1.0-32e33aa-x86_64-linux-gnu.tar.gz -C "$stage"
(cd "$stage" && sha256sum -c SHA256SUMS)
prefix="$HOME/.local/share/sdlc-cli/delivery-20261003"
mkdir -p "$prefix/bin"
install -m 755 "$stage/cicd-cli" "$prefix/bin/cicd-cli"
"$prefix/bin/cicd-cli" --help
export PATH="$prefix/bin:$PATH"
```

## Подключение и откат

Передайте PAT через SDLC_API_TOKEN или продуктовую переменную из [CLI.md](CLI.md);
не помещайте token в аргументы/историю shell. Read scope не отменяет серверную
авторизацию. URL sdlc1 задаётся явно:

```bash
cicd-cli --api-url http://127.0.0.1:7711 --output json project list
```

Для отката CLI используйте сохранённый предыдущий binary/prefix и configuration.
Обновление CLI само не меняет backend. Backend откатывается отдельно по защищённому
rollout manifest на точные прежние pins; автоматический restore данных запрещён.
Известные ограничения старых images, Wiki legacy replay и фактические QA/live
границы перечислены в [CLI_VALIDATION.md](CLI_VALIDATION.md). Production runner/
внешний deploy не сертифицируются. Исторические candidates/checksums сохранены
в предыдущих git revisions и локальных доказательствах, не подменены этой поставкой.

## References

- [CLI](CLI.md)
- [CLI validation](CLI_VALIDATION.md)
- [Base integration](BASE_INTEGRATION.md)
