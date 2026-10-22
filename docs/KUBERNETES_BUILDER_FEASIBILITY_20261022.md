# Strict rootless BuildKit: feasibility gate, 2026-10-22

**FAIL; migration/cutover HOLD.** Source implementation не является подтверждением
нативной сборки. Native evidence предоставил migration owner; эта source lane
runtime не меняла.

Owner запустил pinned `moby/buildkit:v0.33.0-rootless` amd64 manifest
`sha256:9391745530c1812ca16a14554813268c3472953f12a060494efc86b9bc4b0e53`
на выбранном Kubernetes/kernel6.8 с фактическим source security profile:
nonroot1000, no escalation, cap-drop ALL, seccomp RuntimeDefault, readonly rootfs,
bounded state emptyDirs. AppArmor field не добавлялся: nested runtime его не
поддерживает. Первый filesystem failure исправлен узким `TMPDIR=/tmp`.

После этого rootlesskit завершился на `fork/exec /proc/self/exe` с `operation
not permitted` до build/source/registry effect. Доступных audit rows, определяющих
конкретный denying syscall/LSM, нет. Нельзя выдавать отсутствие audit за отсутствие
seccomp/LSM ограничения. Owner evidence: `qa/buildkit-tmpdir-20261022/`, private
Pod/log и safe result остаются вне Git.

[Официальные docs той же версии](https://github.com/moby/buildkit/blob/v0.33.0/docs/rootless.md)
объясняют потребность OCI worker в user/process namespace и mount operations.
`native` snapshotter устраняет overlay/fuse зависимость, но не эти операции.
Kubernetes example upstream использует профиль и process-sandbox ограничения,
не совпадающие с нашим утверждённым контрактом. Поэтому он не доказывает
совместимость strict profile.

Внутри текущего неизменного профиля source/env-only решение не подтверждено.
Ни privileged/unconfined, ни отключение process sandbox, ни смена daemon/socket,
ни повышение host limits не были применены и не являются fallback.

Для закрытия gate требуется отдельное явно принятое решение: квалифицированный
изолированный worker/node и наименьший достаточный security/runtime profile,
с прежними source/task/lease/custody/registry boundaries. Сначала подтвердить
разрешённые userns/process/proc возможности и полный build/push/cancel/recovery,
затем рассматривать rollout. Изменение профиля/worker — новый проверяемый input,
не переименование FAIL в PASS. Обычный OCI executor и recovery source checks
можно квалифицировать независимо; они не снимают этот blocker.
