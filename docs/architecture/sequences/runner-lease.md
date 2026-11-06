# Последовательность выполнения внешнего runner

```mermaid
sequenceDiagram
    participant RN as Forge runner
    participant API as Forge API
    participant K8S as Kubernetes execution
    participant BK as BuildKit worker
    RN->>API: heartbeat: capabilities и свободный slot
    RN->>API: work:poll
    API-->>RN: offer: attempt, lease, generation, environment
    RN->>API: ack и scoped secrets
    RN->>K8S: один START исходной попытки
    opt image-publish
        K8S->>BK: build для исходной попытки
    end
    par До принятого terminal result
        loop
            RN->>API: renew и heartbeat
        end
    and Выполнение
        RN->>K8S: observe / logs
        opt timeout или cancel
            RN->>BK: cancel исходной сборки
            RN->>K8S: остановка исходного execution
        end
        RN->>K8S: доказательство native closure
        RN->>BK: terminal receipt исходной сборки
    end
    RN->>API: complete
    RN->>API: GET канонического receipt
    RN->>RN: освобождение workspace и slot
```

Недоступное или неизвестное состояние сохраняет custody. После потери ответа
сначала выполняется readback; START и внешние действия не повторяются.
Контракт и границы совместимости: [Runner protocol](../../contracts/RUNNER_PROTOCOL.md).
