# Независимое source review narrow inspector QA repair

**QUALIFIED SOURCE PASS, 0 блокеров.** Frozen HEAD `000e2eb6b6d42b81f4cee19eaa23e3f09c917ee6`, tree `a8cdcc995fe3f6c6ec130ff5400b81d733c55286`; предыдущий actual-failed candidate `a57a6854b633c8e272eb4012ff11a0ec125e6fc2`. Checkout чистый. Delta — только существующий LIVE QA script и38-line append checkpoint. Product/CSS, ACCEPTANCE, CI/core/config/site/frozen и physical/3D dependencies/QA побайтно прежние. От main остаются четыре ранее согласованных пути.

QA exact author commit `d61f56d9490a38221bdf4ea046f732619c114dc9`; blob `4c1047bdae711916c962385c081fadf771995304`,37657B/SHA256 `fcd68b338deac36b0dd463e03a1e5a52957b4de73c67f767404cb1f397f4ea8b`. Product остаётся62157B/SHA256 `cf12bae4142980b513375a3a173c4249e457d58e760a366b90153c676446d148`. Reviewed Astra decision — APPROVE_BOUNDED_QA_REPAIR; narrow diff соответствует её границам.

Upper и lower теперь native-scroll один раз на постоянном `#cell-detail`. После await один синхронный callback заново находит current primaryDL/lastDD и вместе читает exact inspector/ID/tick/genome/generation, конкретные rect/style, ancestor clipping, native hit, group/key/value. DOM descendants не удерживаются через await: наружу возвращаются только данные. Lower value берётся из того же callback.

Все strict assertions применяются к настоящему field, не host: complete viewport bounds, size, visible/enabled, clipping и elementFromPoint. Current inspector повторно сверяется с saved API cell. Lower group обязательно Phenotype, numeric raw key/value и прежняя display-value assertion сохранены. Missing/invalid target даёт FAIL. Нет retry, fallback, catch-ignore, нового timer/JS scroll, poll suspension, product/model change или увеличения120s budget. Если один wrapper scroll не покажет current field полностью, QA обязан упасть.

Unchanged prefix до helper —22589B; unchanged tail от rawPhenotypeValue —6553B. Full diff содержит только helper и upper/lower callsites. Реальные Find absent→present, viewport cases/PNG, scroll evidence, API/request/noPOST/model equality, старые holds/clock/poll/source/sandbox/error/cleanup guards сохранены. В shared clipping code clipsX/clipsY используют те же predicates; дополнительное raw ancestor evidence не ослабляет boolean assertions.

Checkpoint точно сохраняет прежний prefix и честно добавляет actual FAIL/qualified trace/approved repair/PENDING. a57 FAIL не превращён в PASS; exact mutating callback остаётся недоказанным. Mobile/later runtime предыдущего run не исполнялись. Persistent-wrapper visibility во всех новых cases ещё не проверена — source review не обещает runtime успех.

Review был независимым и read-only. Browser/HTTP/CI/tests/Cargo/models/probes не запускались; source/Git и прежние frozen reports/scanner не менялись. Responsive cadence/inspector/GL change-impact qualifications предыдущего source review сохраняются; новых native-unhandled/body-hash/postcleanup-ACK claims нет.

Перед новым run root должен закрыть failure archive/readback и дождаться старого CI terminal. Затем обязательны один fresh protected exact-head full CI, новая actual mobile/physical и qualified projection/GL приёмка, affected archive/readback, guarded merge и fresh MAIN/public gates. Любой новый failure/concern возвращает HOLD. LAB/models/causal retries сохраняют HOLD.
