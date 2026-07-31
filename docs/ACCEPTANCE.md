# Приёмочные критерии

Стадия считается сделанной, когда перечисленные тесты существуют и зелены.
Не «когда выглядит правильно».

Критерии в SPEC §13 сформулированы человеческим языком: «баланс сходится 10⁶
тиков; видна конвекция и стратификация». Первое проверяемо, второе — нет.
«Видна» означает, что кто-то посмотрел и остался доволен, и такой критерий
нельзя ни поручить, ни повторить, ни провалить. Здесь те же требования записаны
именами тестов.

Правило, из которого всё следует: **критерий, который нельзя назвать именем
теста, не критерий.** Если требование не сводится к проверке, оно либо
сформулировано слишком расплывчато, либо на самом деле проверяется чем-то
другим — и стоит найти чем.

---

## S0. Среда

### Валидатор отказывается грузить

Восемнадцать отказов. Каждый — класс ошибки, который иначе проявится как
странная динамика через сто тысяч тиков.

```
reaction_unbalanced_by_element_is_rejected
reaction_unbalanced_by_mass_is_rejected
reaction_unbalanced_by_energy_is_rejected
reaction_enthalpy_disagreeing_with_formation_enthalpies_is_rejected
mass_tolerance_above_the_lightest_untracked_substance_is_rejected
reaction_stoichiometry_must_be_positive_integers
substance_without_molar_mass_is_rejected
courant_violation_is_rejected
outflow_bound_violation_is_rejected
scale_overflow_is_rejected
scale_underflow_is_rejected
substance_dynamic_range_over_2e14_is_rejected
reaction_with_unrepresentable_concentration_spread_is_rejected
stoichiometry_in_storage_units_fits_i32
substance_count_over_s_max_is_rejected
reaction_count_over_r_max_is_rejected
every_n_ticks_on_diffusive_field_is_rejected
calibration_path_that_leads_nowhere_is_rejected
```

`courant_violation_is_rejected` и `outflow_bound_violation_is_rejected` — разные проверки. Первая про линейную устойчивость, `max(|u|·dt/dx) ≤ 1`. Вторая про неотрицательность: воксель с расходящимся течением отдаёт сумму по исходящим граням, а не максимум, и условие там строже (SPEC §4.2). Конфиг, проходящий первую и валящий вторую, существует, и он даёт отрицательные количества.

`scale_underflow_is_rejected` — про нижнюю границу разрешения: сценарий
экорежима, запущенный при микрорежимном `dx`, обязан отказаться грузиться, а не
считать воду одной единицей.

Три отказа посередине списка пришли из ADR-039. Два из них —
`reaction_with_unrepresentable_concentration_spread_is_rejected` и
`stoichiometry_in_storage_units_fits_i32` — отличаются от прочих тем, что называют
не вещество, а **пару «вещество, реакция»**; третий,
`substance_dynamic_range_over_2e14_is_rejected`, сравнивает `max_conc` и
`typical_conc` одного вещества и о реакциях не знает. Разброс концентраций внутри
одной реакции ограничен: при `sᵢ · max_conc,ⱼ / typ_conc,ᵢ > 2²⁸·β` (ADR-042) коэффициент
хранения `νᵢ` либо переполняет `i32`, либо оказывается больше всего пула вещества
`i` — и тогда реакция не совершается ни разу, молча. Сообщение об отказе обязано
назвать обе стороны: конфиг, в котором виновата пара, нельзя починить, глядя на
одно имя.

`reaction_with_unrepresentable_concentration_spread_is_rejected` проверяется на
настоящем реестре §2.3, а не на выдуманном. Единственная известная в проекте
несовместность — его собственная: вода и протон в одной реакции при `i32` для
воды. Тест обязан её воспроизводить, иначе он проверяет, что валидатор умеет
отказывать, а не что он отказывает там, где надо.

### Вывод масштабов

Масштабы, экспонента экстента и разрядность не задаются, а выводятся (ADR-039,
ADR-040), поэтому проверять их надо как вычисление, а не как объявление.

```
extent_exponent_is_derived_from_the_scarcest_participant
storage_width_is_derived_not_declared
water_is_stored_in_64_bits_in_the_default_registry
reaction_energy_delta_is_integral_after_load
mass_tolerance_is_derived_not_declared
```

`extent_exponent_is_derived_from_the_scarcest_participant` — прямая защита от
ошибки, ради исправления которой ADR-039 и написан. `e_r`, взятая от самого
обильного участника вместо самого дефицитного, компилируется, грузится и даёт
мир, в котором химия стоит на месте при внешне исправном ledger. Тест подаёт
реакцию с двумя участниками заведомо разной концентрации и смотрит, от которого
получилось число.

`water_is_stored_in_64_bits_in_the_default_registry` выглядит как тест про воду, а
на деле про вывод: если правило разрядности однажды выродится в список имён, он
продолжит проходить. Поэтому рядом стоит `storage_width_is_derived_not_declared` —
он двигает `max_conc` произвольного вещества и требует, чтобы ширина поменялась
следом.

`reaction_energy_delta_is_integral_after_load` — про то, что энтальпия участвует
в реакции наравне с веществом (ADR-041). Объявленная `enthalpy` вещественна, её
коэффициент округляется при загрузке, и тест требует двух вещей сразу: что
округление произошло один раз и до первого тика, и что валидатор назвал
наведённую относительную ошибку числом. Неокруглённый коэффициент дал бы
округление в рантайме на каждом применении — то есть независимое от вещественного,
то есть расходящийся энергетический ledger.

### Сохранение

```
flux_is_antisymmetric
advection_never_produces_negative_amount
advection_alone_conserves_exactly
diffusion_alone_conserves_exactly
transport_of_a_64_bit_substance_conserves_exactly
energy_fold_from_fine_to_coarse_conserves_exactly
reaction_alone_conserves_each_element_exactly
competition_scaling_conserves_each_element_exactly
pressure_relaxation_conserves_exactly
sedimentation_conserves_exactly
boundary_outflow_appears_in_channel_counter
ledger_residual_is_zero_over_1e6_ticks
energy_ledger_residual_is_zero_over_1e6_ticks
channel_counters_do_not_overflow_at_1e7_ticks
```

**`flux_is_antisymmetric` — самый важный тест во всём списке**, и одновременно
самый дешёвый. Свойство: `flux(a, b) == −flux(b, a)` при любых `a`, `b`;
проверяется `proptest` на случайных парах. На нём держится сохранение в
gather-форме (ADR-034), и ничто другое его не ловит: функция потока, написанная
через `floor` вместо округления половин от нуля, компилируется, проходит
grep-проверку хука и выглядит правильно.

`channel_counters_do_not_overflow_at_1e7_ticks` не гоняет десять миллионов
тиков. Он проверяет тип счётчика и подставляет граничное значение: `i32`
переполняется внутри заявленного горизонта, `i64` нет.

Тесты «alone» гоняют один процесс с выключенными остальными — это то, ради чего
ADR-018 требует, чтобы процессы включались и выключались по отдельности.

### Численная верификация

Метод изготовленных решений: берётся гладкая функция, подставляется в уравнение,
невязка добавляется источником, получается точное решение для сравнения.

```
mms_diffusion_converges_at_order_2
mms_advection_converges_at_order_1
diffusion_matches_analytic_gaussian_spread
advection_of_a_step_produces_no_new_extrema
light_attenuation_matches_beer_lambert
temperature_from_enthalpy_round_trips
```

Порядок сходимости проверяется на паре сеток и обязан выходить на ожидаемый, а
не просто «уменьшаться». Ошибка расщепления первого порядка (ADR-036) видна
здесь как порядок ниже ожидаемого — и это единственное место, где её вообще
видно.

`mms_advection_converges_at_order_1` меняет ожидание, если по A-6 будет принят
ограничитель потока.

### Согласованность масштаба

```
integral_quantities_agree_between_dx_and_dx_half
config_schema_declares_no_voxel_dependent_units
```

Первый — тест из SPEC §10. Внимание при его написании: расхождение может быть
законным, из-за численной диффузии схемы первого порядка, которая
пропорциональна `dx` (A-6). Допуск обязан быть выведен, а не подобран, иначе
тест начнёт ловить схему вместо ошибки.

Второй проверяет не значения, а схему: после ADR-035 масштабы выводятся, и в
конфиге не должно остаться ни одного числа, у которого воксель в единицах.

### Детерминизм

```
same_seed_and_config_give_byte_identical_state
different_seed_gives_different_state
reaction_id_is_stable_under_reordering_in_toml
reaction_result_is_independent_of_order_in_toml
stochastic_rounding_is_unbiased_over_1e6_draws
config_hash_ignores_the_calibration_section
```

`reaction_id_is_stable_under_reordering_in_toml` — прямое следствие ADR-027:
если идентификатор берётся от позиции в файле, перестановка двух реакций меняет
поток случайных чисел и, значит, прогон, при неизменной семантике.

`reaction_result_is_independent_of_order_in_toml` — соседний и не тот же самый.
Первый ловит подмену идентификатора, второй — последовательное применение
реакций внутри вокселя: если наличие пересчитывается после каждой, порядок
начинает решать, кто съел субстрат первым. Ошибка невидима, потому что баланс при
этом продолжает сходиться (ADR-041).

### Наблюдаемое поведение

Здесь «видна конвекция» превращается в число.

```
heated_bottom_produces_net_vertical_transport
density_stratification_persists_without_forcing
oxidation_front_forms_at_predicted_depth
```

Это не проверка красоты, а проверка того, что механизм вообще включился.
Порог берётся грубым: конвекция либо переносит вещество вверх на порядок
сильнее диффузии, либо не работает. Точные значения — предмет калибровки, а не
приёмки.

### Производительность

```
ticks_per_second_above_threshold_on_reference_scenario
```

Порог в CI (E-3). Производительность деградирует по пять процентов за PR
незаметно, пока не станет в пять раз медленнее.

---

## Что где живёт

Эталоны для золотых тестов — `tests/golden/`. Каталог защищён хуком
`golden-guard`: перезаписать эталон можно только осознанно, через
`LIMINIS_BLESS_GOLDEN=1`. Инструмент, способный переписать эталон ради зелёного
теста, эталоном не является.

Тесты валидатора и свойств — обычные юнит- и `proptest`-тесты рядом с кодом.
Длинные прогоны на 10⁶ тиков — отдельная цель, не в основном `cargo test`:
в CI гоняется укороченная версия, полная — по расписанию.

---

## Дальнейшие стадии

Критерии для S1′ и далее пишутся, когда стадия становится следующей. Писать их
сейчас — то же самое проектирование вперёд, от которого предостерегает
`NORTH_STAR.md`, только под видом тестов.

Одно исключение стоит зафиксировать заранее, потому что оно влияет на
архитектуру, а не на приёмку: **операциональные критерии перехода** (B-7). Их
надо записать до первых эволюционных прогонов, иначе post-hoc неизбежен —
найдётся красивая структура, и её объявят тем самым переходом.
