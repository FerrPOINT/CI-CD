# Pull Requests и сравнение веток — Forge CI/CD

## 1. Назначение

Минимальный code-review цикл поверх встроенного Git-хостинга: просмотр коммитов и веток, diff между ветками и pull requests со слиянием. Аналог базовых возможностей GitHub/GitLab PR, без reviews с аппрувами, комментариев и CI-статус-чеков (заложено в roadmap).

## 2. API

### Рефы и коммиты

| Method | Path | Назначение |
|---|---|---|
| `GET` | `/api/v1/repos/{repo}/refs` | Ветки и теги: `[{name, sha, target}]` |
| `GET` | `/api/v1/repos/{repo}/commits?branch=main&limit=50` | Коммиты ветки (max 200): `sha, short_sha, author, email, message, date` |

### Сравнение

| Method | Path | Назначение |
|---|---|---|
| `GET` | `/api/v1/repos/{repo}/compare?from=main&to=feature/x` | Diff `merge-base(from,to)..to` |

Ответ:

```json
{
  "from": "main",
  "to": "feature/login",
  "merge_base": "7a4bf4b...",
  "files": [
    { "path": "login.txt", "status": "modified", "additions": 1, "deletions": 0 }
  ],
  "patch": "diff --git a/login.txt b/login.txt\n..."
}
```

### Pull requests

| Method | Path | Назначение |
|---|---|---|
| `GET` | `/api/v1/repos/{repo}/pulls` | Список PR репозитория |
| `POST` | `/api/v1/repos/{repo}/pulls` | Создать PR |
| `POST` | `/api/v1/repos/{repo}/pulls/{number}/action` | `merge` / `close` / `reopen` |

Создание:

```bash
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repos/pr-demo/pulls \
  -H 'content-type: application/json' \
  -d '{
    "repository_name": "pr-demo",
    "title": "Add login page",
    "description": "Implements login UI",
    "source_branch": "feature/login",
    "target_branch": "main"
  }'
```

## 3. Модель данных

Таблица `pull_requests`:

| Колонка | Тип | Описание |
|---|---|---|
| `id` | UUID PK | |
| `repository_name` | TEXT | Репозиторий |
| `number` | INTEGER | Сквозной номер внутри репозитория (`UNIQUE(repository_name, number)`) |
| `title`, `description` | TEXT | |
| `source_branch`, `target_branch` | TEXT | |
| `status` | TEXT CHECK | `open` / `merged` / `closed` |
| `created_by` | TEXT | Заглушка до Phase 1 (auth) |
| `merged_at`, `merge_commit_sha` | | Заполняются при merge |

## 4. Merge в bare-репозитории

Worktree отсутствует, поэтому merge выполняется plumbing-командами:

```text
1. git merge-tree --write-tree -z <target> <source>   # tree + конфликт-инфо
2. git commit-tree <tree> -p <target> -p <source>
       -m "Merge PR #N: <title>"                      # merge commit
3. git update-ref refs/heads/<target> <merge_sha>     # продвижение ветки
```

- `commit-tree` выполняется с идентичностью `Forge CI/CD <forge@localhost>`.
- Конфликт merge-tree завершается неуспешно -> `409 Conflict`, refs не меняются.
- Merge возможен только для `status = open`; повтор -> `409`.
- `close`/`reopen` меняют только БД. Reopen допустим только из `closed`.

Номер PR выдаётся `MAX(number)+1` по репозиторию; без race-защиты (одиночный инстанс, low traffic MVP).

## 5. Frontend

| Страница | Route | Содержимое |
|---|---|---|
| Repository browser | `/repositories/:repo` | Вкладки Коммиты / Ветки / Сравнение / Pull-запросы |
| Compare | `/repositories/:repo/compare?from=&to=` | Выбор веток, файлы со статусами +/-, patch |
| Pull requests | `/repositories/:repo/pulls` | Карточки PR, создание, Слить / Закрыть / Открыть снова |

Всё на общей design system (shadcn/ui, zinc/indigo, ru/en).

## 6. Проверка (e2e)

```bash
# 1. Репозиторий и ветки
curl -fsS -X POST :22801/api/v1/repositories -d '{"name":"pr-demo"}' -H 'content-type: application/json'
git clone http://127.0.0.1:22802/git/pr-demo.git && cd pr-demo
git commit -qm init --allow-empty && git push origin HEAD:main
git checkout -b feature/login; ... ; git push origin feature/login

# 2. Compare
curl -fsS ':22801/api/v1/repos/pr-demo/compare?from=main&to=feature/login'

# 3. PR lifecycle
curl -fsS -X POST :22801/api/v1/repos/pr-demo/pulls -H 'content-type: application/json' \
  -d '{"repository_name":"pr-demo","title":"Add login","source_branch":"feature/login","target_branch":"main"}'
curl -fsS -X POST :22801/api/v1/repos/pr-demo/pulls/1/action \
  -H 'content-type: application/json' -d '{"action":"merge"}'
git ls-remote http://127.0.0.1:22802/git/pr-demo.git   # main указывает на merge-commit
```

## 7. Ограничения и следующий этап

- Нет approve-ревью, комментариев, draft PR, requested reviewers.
- Нет CI-статусов на PR (появится вместе с runner, Phase 5).
- Конфликт не детализируется (просто 409).
- Нет защиты от force-push в target и от удаления ветки в открытом PR.

Дальше: required checks + merge queue, approve-ревью, diff-просмотр по файлам, deletion protection.

## References

- [Git-хостинг](GIT_HOSTING.md)
- [API](API.md)
- [Workflow](WORKFLOW.md)
- [Roadmap](ROADMAP.md)
