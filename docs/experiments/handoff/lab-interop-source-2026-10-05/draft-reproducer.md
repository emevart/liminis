# LAB transport: draft reproducer specification — NOT EXECUTED

Статус: specification only. Не готовый исполняемый repro, не acceptance, не permission для запуска или upstream issue. Нужна отдельная Astra scope decision на основании source audit; текущий LAB HOLD действует.

Цель возможного будущего repro: различить конкретные delivery/interception/consumer lifetime ветви для observed ERR_ABORTED, сохранив все guards. Никакая ветвь заранее не назначается причиной.

- Exact official protected Ubuntu22/Playwright1.61.1/headless-shell1228/version149.0.7827.55, stock launch, chromiumSandbox:true и observed argv guard; реальные версии/commit-source mapping явно сохраняются. Никаких обновлений/flags/kernel bypass.
- Deterministic immutable loopback response fixture: небольшой sanity payload и точные historical5,294,742-byte results JSON с SHA6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a, без нового model run. Sizes/SHA/backend response headers/count источника известны до чтения.
- Задать request/loader/stream IDs и clocks раздельно. Нужны browser native loadingFinished/loadingFailed плюс exact byte count/SHA и explicit cleanup ACK. PW response.finished, parser EOF, stream kDone и aggregate UI сами по себе не являются native completion oracle.
- Request-stage route и response-stage capture — разные cases. Не добавлять Fetch.getResponseBody вместо Network.getResponseBody под прежней подписью. Инициатор observer-induced cancel, если измеряется, отделяется от body consumer/QA cleanup; orphan request IDs или ambiguous clocks не дают causal verdict.
- Plain-network control сейчас НЕ определён безопасно: снятие interception, являющейся частью текущего egress guard, запрещено. Такой case нельзя исполнять, пока protection-equivalent contract отдельно не проверен и не принят. Source audit сам по себе не разрешает изменение guard.
- Не применять JS wrappers/debugger conditional callbacks/GC forcing/timers/response retention как неразмеченную instrumentation. Если когда-либо разрешена instrumentation, её effect и отдельный control входят в bounded scope; текущие источники не дают proof её ненаблюдательности.
- Gate failure сохраняется, неожиданные ERR_ABORTED/NoData не подавляются. Никаких retries until green, scheduler/size/GC claims по одному успешному case или исполнения девяти LAB acceptance gates под видом repro. Количество cases/bytes/time/run budget заранее утверждается отдельно.

До такого решения остаются только сохранённые failure artifacts и source matrix. Draft не меняет PR21, loader, данные, website navigation, CI, oracle или published raw identity.
