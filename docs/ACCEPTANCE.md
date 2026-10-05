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

Семьдесят два отказа. Каждый — класс ошибки, который иначе проявится как
странная динамика через сто тысяч тиков. Порядок тот же, что в таблице §10
`CONFIG_SCHEMA.md`, и число получено пересчётом обоих перечней, а не сложением
дельт из записей журнала: каждая из ADR-056 … ADR-069 берёт базой одно и то же
число девятнадцать.

Три имени пришли с ADR-082 и все три про давление: `theta_max` обязателен
при включённом процессе, пол окна `6·Θ_sup` и потолок по объявленному горизонту
прогона. Четвёртого имени — про `every_n_ticks` на давлении — здесь нет
намеренно, и это следствие того же выбора: правило §10 сформулировано по
**полю**, а не по оператору, поэтому у `every_n_ticks_on_diffusive_field_is_rejected`
расширилась область (третий вход, давление), а имя-двойник рядом с ним
утверждало бы в перечне, что правил два.

**Одно имя ушло и два встали на его место, и это минус один плюс два, а не
переформулировка** (ADR-084 вместе с ADR-083).
`a_lit_scenario_is_refused_until_an_energy_sink_exists` был двухзамковым: он
снимался записью, дающей энергии сток, **и** ответом на разрядность счётчика.
ADR-083 сняла второй замок (`i128`), ADR-084 — первый, объявив стоком S0 уже
построенную грань `exchange`, чей счётчик энтальпии пополняют шаги `c` и `d`.
После обеих замков не осталось, поэтому имя **удалено вместе со своим правилом**,
а не переименовано: напрашивающееся взамен
`a_lit_scenario_is_refused_until_the_counter_width_is_answered` было бы ложно по
посылке в тот самый день, когда его написали бы. На место широкого отказа встали
два узких — `a_lit_scenario_without_an_exchange_face_is_refused` (освещённый
сценарий в запечатанной коробке) и
`a_lit_scenario_whose_steady_state_leaves_the_declared_range_is_refused`
(`i_surface` выше того, что крышка уносит в стационаре). Оба достижимы **ровно с
этого коммита**: пока стоял широкий отказ, он срабатывал раньше любого из них, а
недостижимый сторож есть то же самое, что его отсутствие. Порядок между ними
значим и назначен ADR-084 — сперва «есть ли вообще грань `exchange` и секция
`[boundary.reservoir]`», потом «лежит ли стационар в диапазоне», — потому что
второй предикат считает по `k_ex`, которого без первой проверки может не быть
вовсе.

Двое новых стоят не в конце, а по порядку таблицы §10 — сразу за
`reaction_without_t_vmax_is_rejected`: `every_n_ticks_on_the_light_process_is_rejected`
(ADR-086) и `every_n_ticks_on_the_velocity_field_is_rejected` (ADR-074, применён
ADR-086). Второй ADR-074 назвал и в этот перечень не внёс, а следствие ADR-086
полагало, что внёс, и потому обещало рост на одно имя; выросло на два. Оба про
одно и то же: буфер, писателя и читателя которого разводит расписание,
перестаёт быть `Scratch` и становится межтиковым состоянием класса `Q`, которого
снапшот не несёт.

Последние пятнадцать имён — три класса правил §10, у которых имени теста не
было: ссылочная целостность, область определения и делимость сетки. Правила эти
не новые и не решения — каждое следует из уже принятого, — новыми у них были
только имена, а правило без имени теста по правилу этого документа не критерий.

**Строк в таблице §10 на две больше, чем имён здесь, и это намеренно.**
`initial_layer_naming_an_unknown_substance_is_rejected` и
`initial_layer_side_outside_the_enumeration_is_rejected` пришли с ADR-077 и стоят
ниже, в «Начальных условиях», рядом с остальными именами про сторону слоя: там
они читаются вместе с тем, что стерегут. §10 после них насчитывает семьдесят
четыре строки.

Оба числа выше — и семьдесят два здесь, и семьдесят четыре в §10 — получены
пересчётом перечней в день ADR-090, а не сложением дельт. Пересчёт понадобился:
до него заголовок читался «шестьдесят девять» над семьюдесятью именами, а фраза
про §10 — «семьдесят одну» при фактических семидесяти двух строках. Врал этот
документ в обоих местах согласованно, `CONFIG_SCHEMA.md` был прав; инвариант
«§10 на две больше» держался всё это время, и разошёлся ровно счёт.

Два имени пришли с ADR-090 и оба сравнивают `reaction_id`, поэтому стоят сразу
за `a_duplicate_id_in_any_section_is_refused`: тот сравнивает **имена** на
равенство, `two_reaction_names_folding_to_one_id_are_rejected` — их свёртки, а
`a_reaction_id_landing_in_a_reserved_purpose_window_is_rejected` — вхождение
свёртки в объявленное окно счётчика `purpose` (`NOISE_BASE`, `WORLDGEN_BASE`).
Позиция выбрана по роду сравнения, а не выведена: ADR-090 её не назначает.

Три вещи про них надо знать, и ни одна не следует из имени. **Это отказы
валидатора, а не загрузки.** `config::load` есть `read_to_string` плюс `parse` и
деривации не зовёт вовсе, поэтому `every_scenario_in_the_repository_loads` их не
видит; на поставляемом файле их стережёт `the_shipped_scenario_survives_a_thousand_ticks`
и больше ничто. **В таблицу `every_refusal_names_the_numbers_it_compared` они не
входят** — по той же причине, по которой вне её стоит
`a_duplicate_id_in_any_section_is_refused`: сравниваются не два числа с разными
единицами, а идентификаторы на равенство и на вхождение в окно, и печатать
«сравнили 603427705 с 603427705» значит ничего не сообщить. **Фикстура первого
есть второй якорь смесителя.** Отказ по коллизии печатает общий `rid`, а его
половина с переименованием требует от выжившей реакции ровно ту свёртку, в
которую сворачивается её имя, — то есть держит `name_key` независимо от
`the_name_fold_is_the_same_fold_it_was`.

Последние два имени пришли с ADR-081, и оба про левую часть энергетического
инварианта. `summed_energy_coefficient_disagreeing_with_the_declared_enthalpy_is_rejected`
сверяет **целое** `−Σ ν_s·w_s` с объявленной `enthalpy` в единицах хранения и
отказывает выше `MASS_EPSILON`; он не заменяет
`reaction_enthalpy_disagreeing_with_formation_enthalpies_is_rejected` и не
заменяется им — тот сверяет джоули на моль в `f64` и ловит неверную физику, этот
ловит бит, потерянный при выводе весов, и сценарий может сойтись по первому до
миллионной доли и разойтись здесь на `2.4·10⁻³`. Отказ обязан назвать оба числа и
выход: поднять `max_conc` участников, чей `k_s > k_E`.
`chemical_energy_of_the_domain_past_the_ledger_accumulator_is_rejected` про
`Σ_s |w_s|·n_s` по всей сетке против `i128` накопителя `DomainSums::energy`:
оценка считается в `f64` через `log2` и никогда формированием `i128`, потому что
именно это произведение и переполнилось бы, пока его судят, — а в release
переполнение `i128` не паникует.

```
reaction_unbalanced_by_element_is_rejected
reaction_unbalanced_by_mass_is_rejected
reaction_unbalanced_by_energy_is_rejected
reaction_enthalpy_disagreeing_with_formation_enthalpies_is_rejected
mass_tolerance_above_the_lightest_untracked_substance_is_rejected
substance_lighter_than_its_declared_composition_is_rejected
composition_key_absent_from_conserved_is_rejected
negative_composition_entry_with_nonzero_molar_mass_is_rejected
channel_name_outside_the_enumeration_is_rejected
energy_from_light_is_rejected
reaction_without_t_vmax_is_rejected
every_n_ticks_on_the_light_process_is_rejected
every_n_ticks_on_the_velocity_field_is_rejected
reaction_stoichiometry_must_be_positive_integers
substance_without_molar_mass_is_rejected
substance_without_partial_molar_volume_is_rejected
substance_without_settling_radius_is_rejected
settling_substance_with_zero_partial_molar_volume_is_rejected
courant_violation_is_rejected
outflow_bound_violation_is_rejected
velocity_field_without_u_conv_max_is_rejected
structure_length_below_the_temperature_cell_is_rejected
scale_overflow_is_rejected
scale_underflow_is_rejected
substance_dynamic_range_over_2e14_is_rejected
reaction_with_unrepresentable_concentration_spread_is_rejected
stoichiometry_in_storage_units_fits_i32
energy_scale_incompatible_with_a_reaction_enthalpy_is_rejected
substance_count_over_s_max_is_rejected
reaction_count_over_r_max_is_rejected
every_n_ticks_on_diffusive_field_is_rejected
pressure_without_theta_max_is_rejected
theta_max_below_six_times_the_declared_peak_occupancy_is_rejected
theta_max_beyond_the_declared_run_horizon_is_rejected
field_over_n_max_substeps_is_rejected
thermal_diffusivity_below_the_fastest_substance_is_rejected
t_ref_outside_the_declared_temperature_range_is_rejected
enthalpy_carried_by_diffusion_over_five_percent_is_rejected
exchange_face_without_a_reservoir_is_rejected
reservoir_missing_a_substance_is_rejected
reservoir_concentration_above_max_conc_is_rejected
exchange_velocity_above_the_courant_limit_is_rejected
an_unknown_process_id_is_rejected
a_duplicate_process_id_is_rejected
calibration_path_that_leads_nowhere_is_rejected
a_reaction_naming_an_unknown_substance_is_rejected
km_missing_for_a_reaction_input_is_rejected
km_naming_a_substance_outside_the_inputs_is_rejected
a_catalyst_outside_the_declared_forms_is_rejected
a_requires_window_is_rejected_until_the_gate_exists
light_enabled_without_an_irradiance_is_rejected
a_negative_surface_irradiance_is_rejected
a_modulation_fraction_outside_the_unit_interval_is_rejected
a_modulation_period_that_is_not_a_whole_number_of_ticks_is_rejected
a_modulation_period_under_three_ticks_is_rejected
a_lit_scenario_without_an_exchange_face_is_refused
a_lit_scenario_whose_steady_state_leaves_the_declared_range_is_refused
a_duplicate_id_in_any_section_is_refused
two_reaction_names_folding_to_one_id_are_rejected
a_reaction_id_landing_in_a_reserved_purpose_window_is_rejected
a_reservoir_naming_an_unknown_substance_is_rejected
every_domain_rule_refuses_its_own_violation
a_not_a_number_never_passes_a_domain_rule
t_out_outside_the_declared_temperature_range_is_rejected
stir_period_missing_with_stirring_on_is_rejected
a_calibration_window_that_is_not_an_interval_is_rejected
a_grid_not_divisible_by_its_coarsest_lod_is_rejected
a_periodic_face_without_its_partner_is_rejected
a_settling_substance_in_a_scenario_with_settling_disabled_is_rejected
a_scenario_that_enables_settling_without_a_viscosity_is_rejected
summed_energy_coefficient_disagreeing_with_the_declared_enthalpy_is_rejected
chemical_energy_of_the_domain_past_the_ledger_accumulator_is_rejected
```

Над перечнем стоят три имени, которые сами отказами не являются и без которых он
ничего не значит. `the_worked_example_passes_every_check` гоняет сценарий §12
`CONFIG_SCHEMA.md` целиком: без него весь список зелен у валидатора,
отвергающего всё подряд, — и он же краснеет, когда очередная проверка задевает
собственный пример корпуса. `every_refusal_names_the_numbers_it_compared`
проходит по таблице «фикстура → ожидаемые подстроки» и требует от каждого
сообщения трёх вещей: имя виновника (или пары «вещество, реакция» там, где
ADR-039 требует пару), обе стороны нарушенного неравенства числами и указание,
что менять. `the_validator_refuses_before_it_derives` закрепляет порядок стадий:
конфиг с опечаткой в имени вещества **и** переполненным масштабом отвергается
сообщением про опечатку, потому что §10 прямо описывает исход «реакция,
отвергнутая не тем сообщением», а `derive`, вызванный первым, регулярно даёт
именно его.

**Одно имя перечня стоит `#[ignore]`, и причина записана в самом атрибуте.**
`scale_underflow_is_rejected` — правило за этим именем пережило свою причину:
после ADR-039 разрешение от `dx` не зависит вовсе, и осмысленным остаётся окно
`dx` из ADR-002, которое §13 п. 17 велит либо записать решением, либо снять имя —
но не молча. Три имени, стоявшие тут рядом с ним, ушли:
`reaction_without_t_vmax_is_rejected` и `an_unknown_process_id_is_rejected`
атрибут уже не носят, а `requires_naming_an_unknown_field_is_rejected` снято
записью ADR-073 вместе с правилом — непустой `requires` стал ошибкой загрузки,
поэтому разрешать имя больше не во что, и на его месте стоит
`a_requires_window_is_rejected_until_the_gate_exists`. Умолчания `enabled`
пяти процессов из девяти по-прежнему не назначены ничем (§13 п. 23); свет, поле
скоростей, давление и оседание — те четыре, у которых умолчание назначено
решением (ADR-069, ADR-076, ADR-082, ADR-085).

Тем же ходом ADR-085 снял `a_settling_substance_is_refused_until_g_and_the_medium_density_are_named`
— **вместе с правилом**, как ADR-073 снял своё: `g` и плотность среды объявлены
ключами `[physics]`, поэтому отвергать зерно из-за неназванного числа больше не
за что. На его месте два узких имени, и второе легко потерять. **Зерно при
выключенном оседании** отвергается потому, что осадок, который никогда не
оседает при полностью зелёном корпусе, неотличим от честного нуля — довод,
которым ADR-067 отказал `settling_radius` в умолчании, и цена, которой ADR-085
платит за сохранённое умолчание `enabled = false`. **Включённое оседание без
`physics.mu`** отвергается даже тогда, когда ни одно вещество радиуса не
объявляет: фаза строит скорость для каждой полосы и проверяет вязкость до ветки
нулевого радиуса, поэтому предикат «есть вещество с радиусом» неисполним, а
написанный по радиусу отказ уехал бы с загрузки на сборку тика.

`courant_violation_is_rejected` и `outflow_bound_violation_is_rejected` — разные проверки. Первая про линейную устойчивость, `max(|u|·dt/dx) ≤ 1`. Вторая про неотрицательность: воксель с расходящимся течением отдаёт сумму по исходящим граням, а не максимум, и условие там строже (SPEC §4.2). Конфиг, проходящий первую и валящий вторую, существует, и он даёт отрицательные количества. У знакопеременного поля второе неравенство сторожит не неотрицательность, а отсутствие нового экстремума: энтальпия `H = C_cell·(T − T_ref)` знакопеременна по построению, потому что `T_ref` обязан лежать внутри `[t_min, t_max]` (ADR-062), и «уйти в минус» ей разрешено — что покупает `Σ|C| ≤ 1` на грубой сетке, так это выпуклость, то есть донорная схема выдаёт комбинацию ячейки и её соседей и ячейка не может стать холоднее самого холодного соседа (ADR-087).

**Третьего отказа не будет ни у адвекции, ни у диффузии, и это решено, а не
забыто.** Обе схемы положительны в точной арифметике: у диффузии условие
положительности и условие устойчивости совпадают при `6α ≤ 1`, у адвекции второе
условие уже стоит рядом с первым. Ломает положительность то, чего нет ни в одном
из этих неравенств, — округление потока каждой грани до целой единицы хранения,
— и это не свойство конфига, а свойство состояния поля (ADR-068). Поэтому
недобор принимается с доказанной границей и проверяется по самому буферу, а
блок отказов не растёт ни на одно имя.

Два входа условия Куранта, пришедшие с ADR-067 и ADR-069, **не складываются**.
`u_conv_max` поля скоростей проверяется условием шага `c`, `w_sed` оседания —
собственным одноосевым `w·dt/dx ≤ 1` шага `f`: при расщеплении Ли — Троттера
(ADR-036) каждому оператору достаточно собственного условия. Смещение за тик
складывается из трёх слагаемых — 0.167 вокселя от адвекции, не более 0.167 от
давления, не более одного от оседания, — и это утверждение о модели, а не
неустойчивость.

**Третье условие, диффузионное, принадлежит давлению и с первыми двумя не
складывается тоже** (ADR-082). Донором грани служит весь пул, поэтому
линеаризация около однородного состояния даёт явную диффузию с
`α = Θ/theta_max` — **занятость**, а не разность и не перезаполнение, — а
множитель усиления шахматной моды есть `g = 1 − 12α`, откуда `|g| ≤ 1 ⟺ α ≤ 1/6
⟺ theta_max ≥ 6·Θ`. Ни `max|c| ≤ 1`, ни `Σ|c| ≤ 1` в рабочей точке не связывают:
при `Θ ≈ 1` и `theta_max ≈ 6` они дают `Δθ ≤ 6` и `Δθ ≤ 1`, тогда как схема
колеблется уже на `Δθ = 0`. Третьего `SpeedBound` валидатор поэтому не получает —
складывать нечего, — а связывающая проверка стоит в `config/derive.rs` окном по
`theta_max`.

**Расхождение с замороженной спекой, операционной формой** (ADR-032: SPEC не
правится, расхождение печатается здесь). SPEC §4.2 требует проверить два условия
Куранта и третьего не содержит вовсе, хотя для давления связывает только оно;
именами это `theta_max_below_six_times_the_declared_peak_occupancy_is_rejected`
при загрузке и `the_relaxation_oscillates_once_the_occupancy_exceeds_a_sixth_of_theta_max`
в ядре. Второе расхождение того же решения: SPEC §3 печатает `P = k·(V_occ/V_voxel − 1)`
и отдельно обсуждает необъявленную единицу `k`; после ADR-082 описанной там
величины в модели нет — жёсткость сокращается при выводе подвижности, поле хранит
безразмерное `θ`, и наблюдаемого паскаля не остаётся.

`reaction_without_t_vmax_is_rejected` стоит здесь с ADR-048, а пары в §10
`CONFIG_SCHEMA.md` не имел до этой правки. Теперь имеет, но ключа `t_vmax` схема
по-прежнему не объявляет: правка ADR-048 не применена, и это видно из таблицы.

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
every_substance_occupies_exactly_one_lane
widening_a_substance_does_not_renumber_the_others
mixed_width_storage_matches_uniform_width_storage
a_single_width_registry_allocates_one_field
load_reports_the_restoration_traffic_per_tick
enthalpy_substeps_are_derived_from_thermal_diffusivity
enthalpy_storage_width_is_derived_not_declared
n_max_refusal_names_the_field_and_the_lod_that_fixes_it
enthalpy_at_default_lod_is_rejected_over_n_max
settling_coefficient_is_derived_from_radius_not_declared
stokes_velocity_uses_radius_not_diameter
the_settling_courant_is_compared_the_same_way_in_the_validator_and_in_the_process
the_medium_constants_have_one_source
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

Последние четыре пришли из ADR-056 и проверяют не масштаб, а адресацию: индекс
вещества разрешается в пару «ширина хранения, полоса», причём полоса нумеруется
внутри своего класса ширины, поэтому `lane == s` неверно нигде.
`every_substance_occupies_exactly_one_lane` требует биекции без дыр и без
повторов; `widening_a_substance_does_not_renumber_the_others` запрещает
перенумерацию веществ по ширине, потому что индекс вещества индексирует `nu_sub[]`,
входной вектор генома (ADR-053), метрики и снапшоты;
`a_single_width_registry_allocates_one_field` про законный случай нуля полос в
одном из двух классов.

Четвёртое, `mixed_width_storage_matches_uniform_width_storage`, — несущее и пока
не написано: оно гоняет ядро реакций на разноширинном реестре и на искусственно
одноширинном и требует совпадения результата, а ядра реакций нет. До него ошибка
адресации внутри ядра не ловится ничем — перепутанная полоса читает чужие
количества молча, а ledger сходится, потому что вещество не потеряно, а прочитано
не то.

Последние семь пришли с ADR-057, ADR-061, ADR-062 и ADR-067, и раздела ни одна
из этих записей не назвала — выбран этот, потому что все семь проверяют не
объявление, а вычисление при загрузке.

`load_reports_the_restoration_traffic_per_tick` — про то, что паритет полос есть
свойство конфига, а не кода (ADR-057): порог между `n = 1` и `n = 2` стоит на
`D = dx²/(6·dt) = 1.67·10⁻⁹ м²/с`, H₂S и CH₄ объявлены в четырёх процентах под
ним, и калибровочный сдвиг диффузии меняет счёт копий молча. Загрузчик обязан
напечатать трафик в байтах на тик рядом с числом подшагов, иначе изменение
никак не видно.

`enthalpy_substeps_are_derived_from_thermal_diffusivity` и
`enthalpy_storage_width_is_derived_not_declared` — из ADR-062. Второй несёт
число: поле приращения энергии реакций уезжает в `i64`, воксель растёт с 226 до
**230 байт**, состояние при 128³ — с 474 до **482 МБ**. SPEC §1.2 печатает 226 и
474; спека заморожена (ADR-032), и расхождение живёт здесь.

`n_max_refusal_names_the_field_and_the_lod_that_fixes_it` и
`enthalpy_at_default_lod_is_rejected_over_n_max` стоят рядом с
`field_over_n_max_substeps_is_rejected` из блока отказов, но проверяют не факт
отказа, а его содержание: сообщение обязано назвать поле, его `n` и
`Δlod = ⌈log₄(n/64)⌉`, а второй — что энтальпия при умолчательном `lod = 0`
даёт 84 подшага и отвергается, а не дорожает молча в шестнадцать раз. ADR-061
предписывал второму висеть `#[ignore]` со ссылкой на `CONFIG_SCHEMA.md` §13
п. 9; ADR-062 закрыл этот пункт той же пачкой, так что запускать его есть чем с
первого дня.

`settling_coefficient_is_derived_from_radius_not_declared` и
`stokes_velocity_uses_radius_not_diameter` — из ADR-067, и второй существует
ради множителя четыре. `k = 2r²/9` через радиус, `k = d²/18` через диаметр;
запись `k = r²/18` под именем `settling_radius` даёт скорость ровно вчетверо
меньше истинной, а множитель четыре тише множителя тысяча и переживает
калибровку скорости осадконакопления так же незаметно.

Два имени рядом с ними пришли с ADR-085.
`the_settling_courant_is_compared_the_same_way_in_the_validator_and_in_the_process`
про одностороннее расхождение, невидимое на рабочем примере: валидатор считает
`u·dt/dx` в `f64`, а процесс сворачивает то же число в `Q`, то есть в `f32`, и
сравнивает **его**, потому что именно `Q` получает ядро. Перебором по целым
избыткам плотности от 1 до 4000 кг/м³ при `g = 9.80665` f64-Курант в точке
предела больше единицы **в 2632 случаях**, максимум `1.0000000000000007` при
`Δρ = 1353`. Оставленный в `f64` валидатор отверг бы для двух третей объявляемых
избытков ровно то зерно, которое его же сообщение печатает как самое тяжёлое из
грузящихся, — поэтому он зовёт `settling_velocity` и `settling_courant`, а не
повторяет цепочку вторым текстом, и `transport` бонд оседания повторно не судит.
Свидетель у теста — `Δρ = 1353`, а не учебные 1650: на них f64-Курант в точке
предела равен ровно единице, то есть попадает в ведро, где обе стороны согласны и
свидетельствовать нечем.

`the_medium_constants_have_one_source` — прецедент `the_incident_irradiance_has_one_source`
(ADR-076), и заведён он против дрейфа, который сам же и породил фикстуру: `9.81`
стояло в тесте `process/settle.rs` три записи подряд с честной оговоркой «цитировать
в тесте не значит решать». Тест требует, чтобы фикстура читала
`config::Physics::default()`, и — половина, которую легко потерять, — чтобы обе
двери умолчания давали одно число: отсутствующая секция и секция, написанная без
`g`. При `#[derive(Default)]` они расходятся на 9.80665, оба мира грузятся, оба
консервативны, у обоих обе невязки нулевые, и мир без гравитации просто ничего не
осаждает.

`reaction_energy_delta_is_integral_after_load` — про то, что энтальпия участвует
в реакции наравне с веществом (ADR-041), и содержание у него другое с ADR-081.
`ν_E` больше не округляется от объявленной энтальпии: она **суммируется**,
`ν_E := −Σ_s ν_s·w_s`, то есть сразу в направлении поля. Тест требует трёх вещей.
Что коэффициент — целое, решённое до первого тика. Что **знак** тот: у
экзотермической записи `ν_E` положительна (`+54 144 000` у окисления сульфида
против `−54 144 000`, которые печатал загрузчик до записи), у эндотермической
отрицательна; под старым определением обе носили знак объявленной энтальпии, и
экзотермическая реакция **охлаждала** свою ячейку при закрывающемся ledger'е —
одна и та же `ν_E` стоит по обе его стороны. И что докладываемое число названо:
оно перестало быть `0.5/ν_E` и стало расхождением просуммированного коэффициента
с объявленной энтальпией. Округление при этом никуда не делось — оно переехало на
`w_s` и живёт там, где `k_s > k_E`; проверяется оно на весе, а не на `ν_E`.

### Сохранение

```
flux_is_antisymmetric
flux_is_the_two_named_crossings_composed
a_small_gradient_in_a_large_pool_still_carries_flux
advection_undershoot_stays_within_the_derived_bound
transport_undershoot_stays_within_the_derived_bound
the_undershoot_bound_is_attained_at_the_stability_limit
a_voxel_on_a_closed_boundary_stays_within_the_five_face_bound
undershoot_over_a_tick_is_bounded_by_the_substep_count
a_starved_voxel_is_overdrawn_rather_than_clamped
a_negative_pool_caps_the_extent_at_zero
a_negative_pool_caps_the_competition_coefficient_at_zero
a_guild_mean_is_not_read_below_the_biomass_floor
advection_alone_conserves_exactly
diffusion_alone_conserves_exactly
transport_of_a_64_bit_substance_conserves_exactly
a_process_returns_with_state_n_in_the_front_buffer_for_every_lane
mixed_substep_lanes_diffuse_as_if_each_were_alone
the_restoration_copies_the_smaller_parity_group
a_lane_no_process_touched_is_not_left_a_tick_stale
energy_fold_from_fine_to_coarse_conserves_exactly
the_coarse_face_courant_is_the_fine_flux_over_the_coarse_area
the_coarse_courant_is_the_folded_flux_not_the_folded_courant
a_uniform_flow_has_the_same_speed_on_the_fine_and_the_coarse_faces
shear_across_a_coarse_face_folds_below_either_fine_face
the_covering_faces_are_the_per_axis_shift
the_courant_fold_writes_only_its_own_coarse_faces
the_coarse_face_divergence_is_the_sum_of_the_fine_ones
a_fine_field_within_the_courant_bound_stays_within_it_after_the_fold
diffusing_matter_does_not_move_enthalpy
absorbed_light_appears_in_enthalpy
solar_in_is_credited_the_same_integer_the_fold_added_to_enthalpy
the_solar_slice_is_overwritten_not_accumulated
the_fold_writes_only_its_own_coarse_index_in_every_output
a_tick_without_the_fold_leaves_solar_in_untouched
the_solar_reduction_is_exact_at_the_full_declared_enthalpy_range
load_reports_the_energy_counter_ceiling_in_joules
the_solar_term_does_not_depend_on_the_enthalpy_lod
the_solar_term_scales_with_the_tick
a_dark_scenario_moves_no_enthalpy
the_top_layer_absorbs_nothing_when_the_light_is_off
daily_modulation_preserves_the_period_mean_irradiance_exactly
load_reports_the_ticks_to_the_declared_temperature_ceiling
all_reactions_share_one_competition_coefficient
cell_contributions_are_gathered_not_scattered
cell_energy_ledger_closes_over_apoptosis
reaction_alone_conserves_each_element_exactly
competition_scaling_conserves_each_element_exactly
charge_is_tracked_as_a_conserved_quantity_of_zero_mass
pressure_relaxation_conserves_exactly
pressure_relaxation_reaches_hydrostatic_equilibrium
one_application_of_pressure_reaches_at_most_one_voxel
the_relaxation_oscillates_once_the_occupancy_exceeds_a_sixth_of_theta_max
pressure_crosses_the_domain_in_diffusive_time_not_at_one_voxel_per_tick
an_enabled_pressure_scenario_is_refused_until_step_e_has_an_owner_and_a_boundary
sedimentation_conserves_exactly
settling_out_of_the_top_face_appears_in_boundary_exchange
boundary_outflow_appears_in_channel_counter
a_counter_is_signed_from_the_domains_point_of_view
exchange_face_carries_enthalpy_into_the_energy_counter
the_ghost_cell_is_the_same_value_in_both_buffers
a_domain_reduction_skips_the_ghost_element
an_exchange_face_returns_the_ghost_and_the_ghost_is_its_own_neighbour
an_exchange_axis_paired_with_periodic_is_refused
the_undershoot_bound_counts_the_exchange_face_as_open
ledger_residual_is_zero_over_1e6_ticks
energy_ledger_residual_is_zero_over_1e6_ticks
channel_counters_do_not_overflow_at_1e7_ticks
a_channel_counter_is_as_wide_as_the_domain_sum_it_closes_against
a_channel_counter_holds_ten_million_lit_ticks_at_full_sun
a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon
an_energy_credit_past_the_i64_range_reaches_the_counter_exactly
a_credit_past_the_i128_counter_still_panics_rather_than_wrapping
a_snapshot_round_trips_a_counter_past_the_i64_range
the_shipped_scenario_survives_a_thousand_ticks
the_matter_residual_closes_across_a_tick_with_chemistry_in_it
a_reaction_written_to_the_wrong_lane_breaks_the_matter_residual
a_dropped_reaction_write_breaks_the_matter_residual
the_extent_slice_is_overwritten_not_accumulated
a_tick_without_chemistry_leaves_every_extent_total_at_zero
only_the_reaction_step_declares_transmutes
a_reacting_tick_closes_the_energy_ledger_with_no_channel
the_exchange_face_credits_the_chemical_energy_of_what_it_moves
load_reports_the_chemical_energy_of_the_domain_in_joules
load_reports_the_pressure_relaxation_time_and_the_stability_margin
load_reports_the_lid_credit_per_tick_and_the_counter_margin_in_bits
an_exothermic_reaction_warms_its_cell
a_closed_domain_leaves_every_channel_counter_at_zero
domain_sums_do_not_overflow_for_water_at_256_cubed
```

**`flux_is_antisymmetric` — самый важный тест во всём списке**, и одновременно
самый дешёвый. Свойство: `flux(a, b) == −flux(b, a)` при любых `a`, `b`;
проверяется `proptest` на случайных парах. На нём держится сохранение в
gather-форме (ADR-034), и ничто другое его не ловит: функция потока, написанная
через `floor` вместо округления половин от нуля, компилируется, проходит
grep-проверку хука и выглядит правильно.

**Восемь имён свёртки на грани грубой сетки пришли из ADR-087, и все восемь
сторожат один делитель.** Число Куранта **интенсивно**: шесть десятых и шесть
десятых не есть одна целая две. Складывать надо экстенсивное — объёмный поток
через грань, — и делить потом, поэтому число на грубой грани есть
`2^(−3·lod)·Σ C_мелких`: сложить шестнадцать и разделить на шестьдесят четыре.
Шестнадцать — это `(2^lod)²`, две поперечные протяжённости грубой **грани**;
шестьдесят четыре — `2^(3·lod)`, мелких **ячеек** в грубой, и путать показатели
нельзя. Два промаха не видит ни одна невязка, потому что перенос сохраняет при
любом числе Куранта: среднее арифметическое чисел Куранта завышает поток ровно в
`2^lod = 4` раза, голая сумма — в `2^(3·lod) = 64`. Первый не ловится ни одним
неравенством **никогда** — при потолке валидатора `|C_мелк| ≤ 1/6` осевая сумма
выходит `1/3`, — поэтому его сторожит
`a_uniform_flow_has_the_same_speed_on_the_fine_and_the_coarse_faces` и больше
ничто.

Делитель проверяется **дважды и в двух местах**, потому что промахнуться можно с
двух сторон. `the_coarse_face_courant_is_the_fine_flux_over_the_coarse_area` —
имя ADR-087 — стоит на ядре и проверяет, что делитель применён к правильным
шестнадцати граням; ядру `fold_gain` **приносят**, поэтому выбрать неверный
показатель ядерный тест не может увидеть в принципе.
`the_coarse_courant_is_the_folded_flux_not_the_folded_courant` стоит снаружи
крейта, на `VelocityField::courant_fold_gain`, и проверяет сам показатель — ту
единственную строку хоста, где `2^(−2·lod)` отличается от `2^(−3·lod)` одним
символом. Второе имя ADR-087 не называет, и это расхождение с записью, а не
переименование: журнал append-only, оба имени носят по половине одного
утверждения, и обе половины стоят в этом списке.

`a_fine_field_within_the_courant_bound_stays_within_it_after_the_fold` — восьмое
имя, и ADR-087 его не называет тоже. Оно утверждает вывод про устойчивость
**прямо**, а не рассуждением: при `|C_мелк| ≤ 1/6`, который держит
`speed_bounds` через `outgoing_faces = 6`, свёрнутая грань обязана нести
`|C| ≤ 1/24`, а сумма по двум исходящим граням оси — `≤ 1/12`. Это ровно то
свойство, на котором стоит отказ ADR-087 заводить третий `SpeedBound`: сломанная
свёртка ломает его раньше, чем что-либо увидит валидатор, а список отказов S0 не
растёт ни на одно имя — и это следствие вывода, а не упущение.

Имени `the_velocity_field_is_divergence_free_to_the_declared_tolerance` здесь
нет, и его отсутствие — решение. Никакого объявленного допуска на `div u` в
корпусе не существует: ADR-069 и `kernels/curl.rs` формулируют свойство двумя
половинами намеренно — точно, побитово, на потенциале с носителем в одной ячейке
(`prescribed_velocity_is_divergence_free_bit_for_bit`) и ограниченно округлением
на полном поле, — и числа для второй не печатают. Тест, названный «до
объявленного допуска», обязан либо сослаться на объявленное число, которого нет,
либо выдумать его, а выдуманный допуск есть калибровка поверх дефекта.
Соответствующая половина на грубой сетке —
`the_coarse_face_divergence_is_the_sum_of_the_fine_ones`.

**Восемь имён недобора пришли из ADR-068, и вместе они заменяют одно обещание,
которое было ложным.** Прежнее `advection_never_produces_negative_amount`
обещало неотрицательность и обещать её не могло: положительны обе схемы только в
точной арифметике, а поток каждой грани округляется до целой единицы хранения, и
шесть округлений по половине дают до трёх лишних единиц у любой семиточечной
flux-form схемы. Ограничитель ван Леера этого не меняет — TVD есть свойство
вещественной арифметики. Поэтому имя переименовано в
`advection_undershoot_stays_within_the_derived_bound`, а граница записана
формулой: `new ≥ (минимум по стенсилю) − ⌊f/2⌋ − 3·⌈разброс/2²⁴⌉`, где `f` —
число открытых граней. Абсолютного числа тут назвать нельзя ни одного: три
единицы ниже `2²³`, шесть ниже `2²⁴`, пятьдесят одна на потолке ADR-039, а у
воды `91 198` единиц — что есть `1.8·10⁻⁷` её пула. Имя про закрытую границу
названо через пять граней, а не через «два», ровно поэтому.
`a_starved_voxel_is_overdrawn_rather_than_clamped` уже существует в коде и до
сих пор не был назван ни в одном документе.

Три имени про деление — `a_negative_pool_caps_the_extent_at_zero`,
`a_negative_pool_caps_the_competition_coefficient_at_zero`,
`a_guild_mean_is_not_read_below_the_biomass_floor` — про то, что ломает
отрицательное количество: не разрядность, а три деления. `ξ_max = minᵢ⌊amountᵢ/νᵢ⌋`
при `amount = −3` даёт отрицательный экстент, и реакция идёт назад, сохраняя
вещество, — оба ledger'а сходятся, и не замечает никто. Первое из трёх —
расхождение с замороженной спекой: формула SPEC §5 ломается не транспортом, а
собой.

Четыре имени ADR-057 — про границу процесса, а не про подшаг. Подшаги перестают
трогать указатели поля, а процесс, продвинувший полосы разным числом подшагов,
восстанавливает инвариант сам: копируется **меньшая по числу полос** группа
паритета, не нечётные. Разница считается: копия нечётных стоила бы 100.7 МБ при
128³ против 25.2, потому что самая широкая полоса реестра — вода — стоит именно
на нечётной стороне.

`flux_is_the_two_named_crossings_composed` и
`a_small_gradient_in_a_large_pool_still_carries_flux` — из ADR-060, и второй
несущий. Порядок операций внутри композиции проверить больше нечем: обе формы
антисимметричны, обе сохраняют точно, и `diffusion_matches_analytic_gaussian_spread`
их не различает — разрыв `1.65·10⁻⁹` лежит под любым разумным допуском. Между
тем «сначала концентрации» у воды при `5.1·10¹¹` единиц теряет всё, что меньше
`ulp`, и бугор в 100 000 единиц перестаёт диффундировать совсем: одна ячейка
против двадцати пяти за четыре тика.

`channel_counters_do_not_overflow_at_1e7_ticks` не гоняет десять миллионов
тиков. Он подставляет граничное значение. **Обе ветви теперь доказаны, и прежняя
формулировка — «`i32` переполняется внутри заявленного горизонта, `i64` нет» —
неверна во второй половине** (ADR-083).

По энергии: при `k_E = 67` потолок `i64`-счётчика был `2⁶³/2⁶⁷ = 62.5 мДж`, а
один освещённый тик 128³ при полном солнце даёт `0.16384 Дж = 2.4179·10¹⁹`
единиц — **2.62144 потолка за тик**. За 10⁷ тиков это `2.4179·10²⁶`; счётчик
`i128` держит их с запасом в **39.4 бита**. Граничное значение подставляет
`a_channel_counter_holds_ten_million_lit_ticks_at_full_sun`.

По веществу: вместимость `i64` за весь прогон есть `9.223·10¹⁸` единиц, а
`9.22·10¹¹` — это она же, **делённая на горизонт 10⁷ тиков** (ADR-004); делитель
стоит рядом с числом, потому что без него оно читается как вместимость и
промахивается на семь порядков. Ветвь достижима **при работающем сквозном
потоке** — жерло или химия внутри, вынос через `BOUNDARY_EXCHANGE`, — а крышкой
самой по себе не достижима вовсе: счётчик знаковый и хранит нетто, поэтому сутки
симметричного обмена выглядят как отсутствие обмена. Сквозной поток в один
процент пула поверхностного вокселя есть `8.389·10¹³` единиц за тик и исчерпывает
`i64` на тике `1.0995·10⁵` — на одиннадцати процентах самого короткого
объявленного горизонта. Это подставляет
`a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon`.

**Измеренный вход у этой пары появился с ADR-084, и он не солнечный.**
Поставляемый `h2s-oxidation.toml` переполнял `BOUNDARY_EXCHANGE` **на седьмом
тике в темноте**: мир стартует при `H = 0`, то есть при `T_ref = 298.15`,
резервуар объявлен при `t_out = 288.15`, и десять кельвинов разницы гонят через
крышку измеренные `1.4058·10¹⁸` единиц за первый тик — `0.152` потолка `2⁶³` за
тик, — ни света, ни излучения, ни одного включённого процесса кроме диффузии.
Рядом с этим числом обязан стоять честный потолок накопления, потому что соблазн
умножить его на горизонт велик и **неверен**: поток крышки — затухающий
переходник, а не постоянный приток, коробка релаксирует к `t_out` за
`L/k_ex = 480` тиков, и весь интеграл ограничен её собственной энтальпией над
`t_out` — `4.1710·10⁶ Дж/(м³·К) · 1.10592·10⁻⁷ м³ · 10 К = 4.613 Дж`, то есть
`6.807·10²⁰` единиц, **около семидесяти бит со знаком**. Тёмная коробка
доказывает измерением, что `i64` узок (73.8 потолка против одного), и **не
доказывает, сколько бит нужно**; требуемую ширину задаёт солнечный случай выше.
Различие несёт вес и для отвергнутых вариантов A-20: на семидесяти битах дешёвые
ответы — отдельный масштаб счётчика, периодическое сведение — остались бы
живыми, и убивает их солнце, а не крышка.

Корпусное «10⁶ тиков по тысяче единиц дают 10⁹» из `QUANTITIES.md` §3 —
заглушка, а не худший случай, и она сохранена под своим именем: половина её
довода («`i32` безнадёжен, и молча») верна и остаётся. Операционная форма,
делающая потолок видимым, — строка отчёта загрузки
`load_reports_the_energy_counter_ceiling_in_joules`; после ADR-083 она печатает
`2¹²⁷/2^k_E`.

Шесть имён ADR-083, и первое из них сторожит само решение — ширина элемента
правой стороны равна ширине элемента левой:

```text
a_channel_counter_is_as_wide_as_the_domain_sum_it_closes_against
a_channel_counter_holds_ten_million_lit_ticks_at_full_sun
a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon
an_energy_credit_past_the_i64_range_reaches_the_counter_exactly
a_credit_past_the_i128_counter_still_panics_rather_than_wrapping
a_snapshot_round_trips_a_counter_past_the_i64_range
```

`crediting_more_solar_than_the_counter_holds_is_refused_not_wrapped` снят вместе
с конфигурацией, которую называл: после ADR-083 «больше солнца, чем держит
счётчик» недостижимо никаким солнцем в объявленном размахе, и имя стало бы
зелёным всегда и ни о чём. Его отказ переехал на границу `i128` внутрь
`a_credit_past_the_i128_counter_still_panics_rather_than_wrapping`.

**`a_counter_is_signed_from_the_domains_point_of_view` существовал в коде
задолго до того, как его назвал документ, и это ровно то, против чего написано
правило этого файла.** Соглашение о знаке — счётчик хранит **приращение
домена**, входящий поток положителен, исходящий отрицателен, `Δ(домен) ==
Σ(счётчики)` читается как напечатано — ратифицировано ADR-084 и стоило **нуля
строк логики**: код угадал верно и честно пометил выбор `TODO`, потому что
ADR-059 объявил в одной записи два взаимоисключающих соглашения. Ноль строк
логики означает, что держат соглашение тесты и больше ничто, поэтому их надо
называть по счёту: этот — изнутри крейта, на обе стороны и на **хранимое** число,
а не только на невязку; `boundary_outflow_appears_in_channel_counter` — снаружи,
и он сильнее, потому что судит знак, выбранный диспетчеризованным вызовом, а не
поданный тестом. Больше нигде обратное соглашение упасть не может: на закрытом
домене оба дают ноль, а согласованное отрицание кредита и формулы невязки не
двигает невязку вовсе.

`load_reports_the_lid_credit_per_tick_and_the_counter_margin_in_bits` — цена
стока, сделанная измеримой (ADR-084). Загрузка печатает проводимость крышки,
потолок поглощённого потока, энтальпию ghost-ячейки и потиковое зачисление с
запасом счётчика **в битах**. Имя названо так, а не «тик, на котором счётчик
переполнится», и это не косметика: после ADR-083 счётчик `i128`, и тика
переполнения у крышки нет ни при каком объявленном мире (`1.4·10¹⁸` за тик против
`2¹²⁷` дают `1.2·10²⁰` тиков, а весь интеграл затухающего переходника упирается в
семьдесят бит), — строка, печатающая «тик переполнения», печатала бы число,
которого не бывает, то есть была бы обманкой ровно того класса, за который
ADR-083 убил `crediting_more_solar_than_the_counter_holds_is_refused_not_wrapped`.
Само зачисление — **верхняя граница**, а не число прогона, и тест требует именно
её: зазор затухает внутри тика по подшагам, поэтому воспроизвести цифру прогона
загрузчик не может, а `alpha_ex·|H_out|·(грубые ячейки обменивающихся граней)` её
ограничивает сверху и на поставляемом сценарии промахивается меньше чем на
процент.

**`the_shipped_scenario_survives_a_thousand_ticks`** — то, чего у приёмки не было
по **роду**, а не по покрытию: `every_scenario_in_the_repository_loads` проверяет
загрузку, и ни один тест корпуса не крутил ни одного сценария. Поэтому паника на
седьмом тике поставляемого `h2s-oxidation.toml` прошла мимо всех тестов. Тест
строит мир тем же путём, каким его строит прогон (`serve::build`, иначе крышка
торгует с незасеянным резервуаром), и после каждого из тысячи тиков сводит
инвариант **собственным** вызовом `assert_closed`, а не полагаясь на
`#[cfg(debug_assertions)]` внутри `Tick::advance`. Сценарий один и в единственном
числе намеренно: `hello.toml` проходит загрузку и не строится — умолчание
`exchange` на `z_max` без `[boundary.reservoir]`.

**`a_tick_without_the_fold_leaves_solar_in_untouched` носит `#[ignore]`, и
причина записана в атрибуте.** ADR-075 называл его конфигурацию достижимой —
реестр с выключенными реакциями не идёт ни шагом `h`, ни шагом `i′`, — и той же
записью сказал, что места диспетча шага `i′` не создаёт: `process::credit_solar`
приезжает без вызывающего. Значит `SOLAR_IN` не пополняет ни один реестр, срез
`World::solar_in` не пишет никто, и обе половины теста держатся при любой
реализации — в том числе при редукции, унесённой в фазу 5, то есть при ровно том
варианте, который тест заведён отличать. Имя возвращается в строй в тот день,
когда шаг `i′` попадает в диспетч; ослабить его формулировку значило бы оставить
на этом месте видимость живого сторожа.

**Шесть имён ADR-080 — про второе слагаемое правой части вещественного
тождества**, и SPEC §2.1 с ними расходится: там правая часть одночленная,
`Δ(Σ поля + Σ клетки) == Σ(потоки по каналам)`, а в коде она стала
`Δnₛ == Σ_c credited(c, s) + Σ_r ν_{r,s}·Ξ_r`. Спека заморожена (ADR-032),
расхождение живёт здесь. Так же расходится §4.1: `invariant` отвечает тремя
ответами, а не двумя, — третий `Transmutes`, и его объявляет один шаг `h`.

`the_matter_residual_closes_across_a_tick_with_chemistry_in_it` — то, ради чего
всё остальное: до ADR-080 `Ledger::assert_closed` падал на первом же тике с
реакцией, и это была вещественная половина замка на шаге `h`.

Несущие — второе и третье. `a_reaction_written_to_the_wrong_lane_breaks_the_matter_residual`
падает от ошибки, ради невидимости которой отвергнут главный вариант записи —
невязка по сохраняемым величинам вместо повеществной: на поставляемом реестре
матрица состава имеет ранг 1 при пяти именах, и перенос количества между двумя
веществами равного состава проецируется в ноль.
`a_dropped_reaction_write_breaks_the_matter_residual` падает от ошибки, ради
невидимости которой отвергнута сильнейшая альтернатива — восстановление `Ξ`
на хосте из доменных дельт: та проверка слепа ровно на `span{ν}`, а потерянная
запись целого вокселя даёт `Δn`, пропорциональное `ν`.

`the_extent_slice_is_overwritten_not_accumulated` — про ADR-045: ядро
перезаписывает свою ячейку, и накопление вернуло бы прошлый тик через ту самую
дверь, которая заведена его ловить. `a_tick_without_chemistry_leaves_every_extent_total_at_zero`
сторожит обнуление в `begin_tick`, и экстент в нём **подсаживается** до прогона:
таблица, обнуляемая только конструктором, зелена при любой записи теста, где её
не трогали. `only_the_reaction_step_declares_transmutes` сторожит **обе** оси:
ни один процесс S0 не объявляет `Transmutes` по энергии — там ADR-081 оставляет
шагу `h` буквальный `Conserved`, потому что взвешенная левая часть от реакции не
меняется вовсе.

Цена названа числом и она на воксель. ADR-086 пересчитал её целиком: под
правилом «бюджет на воксель считает память прогона, а не состояние» (ADR-045,
ADR-067, ADR-080) прибавляются **три** повоксельных буфера, а не один —
`face_courant` 12 Б (`3·4`, плюс 12 Б на три призрачные грани прогона), `light`
4 Б и срез экстента `4·R` Б. Итого **250 байт на воксель при `R = 1`** и **502
при `R_MAX = 64`**; при 128³ это **524.3 МБ** и **1 052.8 МБ**, при 48³ —
**27.65 МБ** и **55.52 МБ** против 25.44 МБ базы. Против замороженного числа
SPEC §1.2 в 226 Б и 474.0 МБ это **+10.6 %** и **+122.1 %**, и это расхождение с
замороженной спекой: 226 байт не правятся (ADR-032), операционная форма живёт
здесь. Строка спеки «при 256³ — 3.8 ГБ» читается после этого как 4.19 ГБ при
`R = 1` и 8.42 ГБ при `R = 64`. Длина среза экстента равна `n_reactions`, а не
`R_MAX`, и это единственное, что отодвигает обрыв.

**Седьмого имени ADR-080 здесь нет, и это не пропуск.**
`load_reports_the_bytes_the_extent_slice_costs` требует строки в отчёте загрузки,
то есть правки `config/derive.rs` и поля в `Derived`; в корпусе его пока нет.
Числа выше живут в доке `kernels::react::react_voxel` и в `version.rs`, а имя
остаётся долгом — по правилу этого документа его нельзя вписать в перечень
раньше теста.

Последние два пришли из ADR-059 и, по его же словам, самые дешёвые и самые злые.
`a_closed_domain_leaves_every_channel_counter_at_zero` гоняет настоящий транспорт
над тором и требует двух вещей сразу, а не одной: обе невязки — точные нули, и
все шесть каналов не тронуты ни по одному веществу. Первое сохранилось бы и у
процесса, зачисляющего поток через **внутреннюю** грань в `BOUNDARY_EXCHANGE` —
кредиты по домену взаимно сократились бы, — второе нет.

`domain_sums_do_not_overflow_for_water_at_256_cubed` называет и вещество, и сетку
намеренно. Взять худшим случаем 32-битное вещество — именно так возьмёт первый,
кто станет считать, — значит взять не тот: при потолке `2³¹/8` на воксель его
доменная сумма при 128³ есть `5.63·10¹⁴`, что переполняет `i32` ровно в `2¹⁸` раз
и оттого выглядит как довод в пользу `i64`, но остаётся в 16 383 раза ниже
`i64::MAX`. На таком тесте разрядность накопителя не проверяется вовсе: он зелен
и при `i64`, и при `i128`. Худший случай — вода, единственное 64-битное вещество
реестра (ADR-040): `5.12·10¹¹` единиц на воксель, при 256³ — цели S5 из SPEC §13
— доменная сумма `8.59·10¹⁸`, то есть **93% `i64`**. Запас исчезает не в
теоретическом пределе, а на числе, уже записанном в дорожной карте, поэтому
редукция ведётся в `i128`, а тест обязан гоняться именно на воде и именно при
256³. Левая сторона при этом не исчерпывается `amount[]`: гильдийные поля и
`struct_mass` таблицы клеток — тоже моли `BIOMASS` (ADR-059), и их вклад тест
подаёт отдельно.

`sedimentation_conserves_exactly` — имя не новое, но с ADR-067 у него появилось
содержание: один оператор, одна ось, точная антисимметрия потока через грань.
Второго имени для той же проверки запись не заводит, и `settling_alone_…` был бы
дублем, а не соседом: процессом-одиночкой тест делает конфиг, а не имя.
Рядом стоит `settling_out_of_the_top_face_appears_in_boundary_exchange`:
всплывающее вещество (`ρ̄ᵢ < ρ_среды`, та же формула с обратным знаком) покидает
домен через `z_max` и обязано быть дебетовано каналом против ghost-ячейки
резервуара.

Пять имён вокруг `boundary_outflow_appears_in_channel_counter` — про механизм
ghost-ячейки ADR-059, и каждое ловит ошибку, которую **ни одна невязка не видит**.
`the_ghost_cell_is_the_same_value_in_both_buffers`: подшаги чередуют направление
(ADR-057), поэтому ghost, засеянный в один буфер, делает крышку бесконечным
стоком через подшаг — ноль есть законное количество, поток остаётся
антисимметричным, счётчик записывает ровно то, что утекло, и обе невязки сходятся
точно. `a_domain_reduction_skips_the_ghost_element`: резервуар постоянен и
сокращается в разности `after − before`, поэтому врут только абсолютные суммы, а
вместе с ними экспорт объёма и полоса floor/peak worldgen.
`an_exchange_face_returns_the_ghost_and_the_ghost_is_its_own_neighbour`: без
раннего возврата `Grid::coords` декодирует `n_voxels` как координату за концом Z,
и в release ответ зависит от экстентов сетки.
`an_exchange_axis_paired_with_periodic_is_refused` — тот же довод, что у
полупериодической оси, на шаг дальше: у периодической половины сосед за гранью
есть, у обменной он ghost. `the_undershoot_bound_counts_the_exchange_face_as_open`:
у вокселя верхнего слоя при `z_max = exchange` шесть **открытых** граней, а не
пять, и граница ADR-068 для него на единицу шире; имя
`a_voxel_on_a_closed_boundary_stays_within_the_five_face_bound` этого не видит,
оно про закрытую крышку.

`charge_is_tracked_as_a_conserved_quantity_of_zero_mass` пришпиливает тестом
обещание ADR-025, которое до ADR-064 было прозой: `charge = 0.0` в `conserved` и
знаковый `charge = -2` в `composition`. Раздела ADR-064 для него не назвал;
выбран этот, потому что заряд — сохраняемая величина, и проверяется он тем же
элементным балансом.

Тесты «alone» гоняют один процесс с выключенными остальными — это то, ради чего
ADR-018 требует, чтобы процессы включались и выключались по отдельности. С
ADR-065 их конфиги дорожают: перечень процессов замкнут и материализуется
целиком, поэтому выключить остальные можно только выписав `enabled = false` на
каждый — три строки на процесс, двадцать четыре на конфиг.

### Численная верификация

Метод изготовленных решений: берётся гладкая функция, подставляется в уравнение,
невязка добавляется источником, получается точное решение для сравнения.

```
mms_diffusion_converges_at_order_2
mms_advection_converges_at_order_2_on_monotone_data
flux_limiter_falls_back_to_first_order_at_zero_gradient
diffusion_matches_analytic_gaussian_spread
advection_of_a_step_produces_no_new_extrema
light_attenuation_matches_beer_lambert
temperature_from_enthalpy_round_trips
exchange_inflow_falls_back_to_first_order
abiotic_reaction_proceeds_with_empty_catalyst
abiotic_rate_is_independent_of_every_catalyst_field
catalyzed_rate_equals_abiotic_rate_times_catalyst_concentration
```

`temperature_from_enthalpy_round_trips` перестал быть неисполнимым: ADR-062 дал
ему и масштаб, и сетку. Гонять его надо на **грубой** сетке поля энтальпии —
температура величина грубой ячейки, шестьдесят четыре мелких вокселя делят одно
`T`, и `q10` реакции читает температуру накрывающей ячейки, — иначе непонятно,
какой состав он замыкает.

`exchange_inflow_falls_back_to_first_order` — про то, что откат ограничителя на
границе получается даром: при втекании донором служит ghost-ячейка, ячейкой выше
по потоку от неё оказывается она же, числитель отношения `r` обращается в ноль и
`φ(0) = 0`. Цена названа честно: на плоскости втекания численная диффузия
возвращается к `1.25·10⁻⁹ м²/с` против молекулярной кислородной `2.1·10⁻⁹` — но
на 0.78% домена и в один слой.

Три имени ADR-063 — про условную единицу `vmax`, и раздела запись не назвала;
выбран этот, потому что все три сверяют скорость с аналитическим отношением, а
не проверяют сохранение. Первое ловит исход «катализатор нулевой концентрации»:
абиотическая реакция с законным `vmax` обязана давать `ξ > 0`, иначе вся химия
S0 стоит, обе половины инварианта сходятся на `0 = 0` и падать нечему. Второе
ловит фантом: изменение любого поля катализа не меняет `ξ` абиотической реакции.
Третье закрепляет развилку с обеих сторон — при одинаковом числе `vmax`
каталитическая скорость больше абиотической ровно во столько раз, какова
концентрация катализатора.

Порядок сходимости проверяется на паре сеток и обязан выходить на ожидаемый, а
не просто «уменьшаться». Ошибка расщепления первого порядка (ADR-036) видна
здесь как порядок ниже ожидаемого — и это единственное место, где её вообще
видно.

Адвекция ждёт второго порядка, потому что по ADR-054 принят ограничитель ван
Леера. Оговорка в имени теста несущая: изготовленное решение обязано быть
**монотонным вдоль оси прогонки**. TVD-ограничители деградируют до первого
порядка на экстремумах, и на решении с максимумом порядок выйдет между единицей
и двойкой — то есть тест начнёт мерить положение экстремума, а не схему.

`flux_limiter_falls_back_to_first_order_at_zero_gradient` — про частный случай,
который иначе обнаружится делением на ноль в проде: при нулевом градиенте
отношение `r` не определено, и **обе стороны грани** обязаны откатиться
одинаково. Разный откат ломает антисимметрию, и ловит это уже
`flux_is_antisymmetric`, но отдельный тест называет причину.

### Согласованность масштаба

```
integral_quantities_agree_between_dx_and_dx_half
config_schema_declares_no_voxel_dependent_units
```

Первый — тест из SPEC §10, но **не в той форме, в какой §10 его описывает**, и
это первое зафиксированное расхождение с замороженной спекой (ADR-054). Там
сказано «расхождение означает, что воксель просочился в формулу»; после принятия
ограничителя ван Леера это слишком сильно. Численная диффузия падает как `dx²`, а
не как `dx`, поэтому расхождение при удвоении разрешения обязано не исчезать, а
**уменьшаться вчетверо**. Правильная форма теста — проверка порядка, а не
равенство с допуском: допуск пришлось бы подбирать, чего §10 сам же и запрещает,
а порядок выводится из схемы.

Смысл при этом сохраняется полностью. Воксель, просочившийся в формулу, даёт
расхождение, которое при измельчении сетки **не** падает как `dx²` — либо не
падает вовсе, либо падает не с тем порядком. Тест ловит ровно это.

Второй проверяет не значения, а схему: после ADR-035 масштабы выводятся, и в
конфиге не должно остаться ни одного числа, у которого воксель в единицах.

### Детерминизм

```
same_seed_and_config_give_byte_identical_state
different_seed_gives_different_state
different_seed_changes_the_rounding_on_identical_state
run_key_is_derived_from_both_halves_of_the_seed
the_name_fold_is_the_same_fold_it_was
names_differing_in_one_byte_fold_to_unrelated_ids
the_empty_name_folds_to_a_fixed_nonzero_id
seed_in_the_scenario_file_is_rejected
reaction_id_is_stable_under_reordering_in_toml
reaction_result_is_independent_of_order_in_toml
stochastic_rounding_is_unbiased_over_1e6_draws
config_hash_ignores_the_calibration_section
the_projection_covers_every_simulated_key
the_canonical_form_reloads_to_the_same_hash
an_omitted_process_section_hashes_as_the_full_default_roster
the_canonical_form_names_every_process_in_the_roster
an_omitted_physics_section_hashes_as_earth_and_fresh_water
```

Три имени ADR-058 стоят здесь потому, что сид дошёл до генератора: `rand`
получил четвёртый счётчик `run_key`, и через него расходятся по сиду и химия, и
генерация начальных условий. `different_seed_gives_different_state` до этого
провалить было нечем — менять от сида было нечего.
`run_key_is_derived_from_both_halves_of_the_seed` не косметика: порядок
вложения несимметричен намеренно, `mix(lo) ^ mix(hi)` схлопнул бы сиды, у
которых половины переставлены. `seed_in_the_scenario_file_is_rejected` почти
бесплатен — `deny_unknown_fields` уже делает такую строку ошибкой, — и
существует затем, чтобы это было решением, а не совпадением; это же операционная
форма расхождения с SPEC §12.4, которая говорит «seed в конфиге».
`different_seed_gives_different_state` при этом есть утверждение о **конкретной
паре** сидов, а не теорема: `u64 → u32` схлопывается, и внутри галереи из десяти
тысяч сидов вероятность коллизии `1.2%`.

Три имени ADR-090 стоят рядом с ними, потому что это та же конструкция во второй
раз: `run_key` сворачивает внешнюю идентичность прогона в счётчик `rand`,
`name_key` — внешнюю идентичность реакции в третий счётчик того же броска.
`the_name_fold_is_the_same_fold_it_was` печатает якоря числами, и это не
украшение: свёртка есть семантика мира ровно того же уровня, что генератор, —
поменяй в ней раунд, и каждый бросок каждого прогона поменяется вместе с ним, —
поэтому тест обязан краснеть от правки смесителя, а не пересчитываться под неё.
`names_differing_in_one_byte_fold_to_unrelated_ids` требует лавины: имена
реестра различаются на байт куда чаще, чем случайно, и свёртка, у которой
соседние имена дают соседние числа, обесценивает третий счётчик, ничего при этом
не ломая. `the_empty_name_folds_to_a_fixed_nonzero_id` про вырожденный вход —
`mix(START)` без единого раунда, — и он же держит `START` от подмены нулём.
Второй якорь того же смесителя стоит в другом месте и назван выше: фикстура
переименования в `two_reaction_names_folding_to_one_id_are_rejected`.

`reaction_id_is_stable_under_reordering_in_toml` — имя ADR-027, написуемое
только с ADR-090, и живёт оно в `crates/liminis-core/tests/acceptance_reactions.rs`,
где стоит единственная в `tests/` фикстура с настоящим реестром реакций. Путь
через `config::validate` — условие, а не удобство: `config::load` деривации не
зовёт, поэтому `rid` в него не попадает вовсе. Переставляет тест строки, но не
добавляет их, и это его объявленное слепое пятно: `rid`, зависящий от всего
реестра, под перестановкой устойчив и разъедется в день, когда сценарий получит
ещё одну реакцию.

`the_projection_covers_every_simulated_key` и
`the_canonical_form_reloads_to_the_same_hash` — из ADR-066, и первый ловит то,
чего не ловит компилятор: исчерпывающая деструктуризация не отличает `field: x`
от `field: _`. Тест собирает множества путей ключей у `Config` и у проекции и
требует, чтобы разность в точности равнялась константе `NOT_HASHED`. Полон он
настолько, насколько полна его фикстура: пустой массив таблиц печатается как
`substance = []` и не даёт ни одного вложенного пути, поэтому фикстурой обязан
быть сценарий §12 `CONFIG_SCHEMA.md` целиком — не меньше одной записи каждого
массива, включая `[[calibration]]`.

`an_omitted_process_section_hashes_as_the_full_default_roster` и
`the_canonical_form_names_every_process_in_the_roster` — из ADR-065. Первый есть
§11 п. 2, распространённый на секцию, полного состава которой в файле нет:
конфиг без записи про диффузию и конфиг с выписанным умолчанием обязаны дать
побайтово равные хеши. Второй — единственное, чем подкреплено обещание «видно,
что именно получилось»: `config::canonical` публична, но наружу её не выводит
никто, и без флага `--print-canonical` пользователь, забывший запись, не видит
вообще ничего.

`an_omitted_physics_section_hashes_as_earth_and_fresh_water` — то же правило,
применённое к секции целиком (ADR-085, ADR-065), и второе его утверждение важнее
первого. Равенство хешей выполнилось бы и в мире, где `[physics]` выброшена из
проекции вовсе, поэтому тест разбирает каноническую форму обратно в `toml::Value`
и требует наличия `physics.g = 9.80665` и `physics.rho_medium = 1000.0` у
сценария, который не написал ни того, ни другого. Третье утверждение — зеркало:
`physics.mu` в канонической форме **отсутствует**, пока её не объявили, по
образцу `theta_max`, потому что материализованное `Some(_)` было бы умолчанием,
которого решение не назначало. Падает он при `#[derive(Default)]` для `Physics`
(пропущенная секция даёт `g = 0`), при `Option<Physics>` в `Config` (секция не
попадает в канонический вид вовсе) и при материализации после хеширования.

`reaction_id_is_stable_under_reordering_in_toml` — прямое следствие ADR-027:
если идентификатор берётся от позиции в файле, перестановка двух реакций меняет
поток случайных чисел и, значит, прогон, при неизменной семантике.

`reaction_result_is_independent_of_order_in_toml` — соседний и не тот же самый.
Первый ловит подмену идентификатора, второй — последовательное применение
реакций внутри вокселя: если наличие пересчитывается после каждой, порядок
начинает решать, кто съел субстрат первым. Ошибка невидима, потому что баланс при
этом продолжает сходиться (ADR-041).

### Начальные условия

```
same_seed_gives_the_same_initial_state
different_seed_gives_a_different_initial_state
worldgen_respects_declared_max_conc
layer_sides_are_anticorrelated_across_the_boundary
the_layer_side_does_not_change_with_the_seed
a_substance_declared_uniform_has_no_layer_step
the_canonical_form_names_the_layer_side_of_every_substance
initial_layer_naming_an_unknown_substance_is_rejected
initial_layer_side_outside_the_enumeration_is_rejected
the_initial_band_keeps_every_substance_strictly_above_zero
```

Процедурная генерация из шума (SPEC §12.4, ADR-021, ADR-058). Первые два имени —
не синонимы двух имён из «Детерминизма» этажом выше, и разделены намеренно.
`same_seed_and_config_give_byte_identical_state` и
`different_seed_gives_different_state` — про **прогон**: совпадают ли два мира
после тиков. Эти два — про нулевой тик, и проверяются раньше, чем существует
хоть один процесс. Расхождение по сиду, возникшее в генерации, и расхождение,
возникшее в стохастическом округлении химии, лечатся в разных файлах, а
провалившееся имя обязано называть файл.

`same_seed_gives_the_same_initial_state` сверяет все буферы мира, а не только
количества, и вдобавок требует, чтобы свёртка сида жила на хосте: мир,
сгенерированный из младшей половины сида, не есть мир, сгенерированный из его
ключа. `different_seed_gives_a_different_initial_state`, как и старший брат, есть
утверждение о **конкретной паре** сидов, а не теорема, и по той же причине:
`u64 → u32` схлопывается, `1.2%` на галерею из десяти тысяч (ADR-058). Он же
требует доли разошедшихся вокселей, а не `assert_ne!`: одного несовпавшего
вокселя хватило бы, а доля обязана сидеть у единицы — и он же ловит вариант,
отвергнутый ADR-058, при котором ключ, свёрнутый в первый счётчик, даёт те же
числа с переставленными вокселями.

`worldgen_respects_declared_max_conc` стоит здесь, а не в «Выводе масштабов»:
проверяется не вывод, а его соблюдение. `max_conc` есть жёсткий потолок прогона
(ADR-041), а не оценка, и мир, стартовавший над ним, есть неверный старт, а не
начальное условие. Проверки на границе тика, обещанной ADR-041, не написано,
поэтому это имя — единственное, что стоит между сценарием и переполнением,
которое проявится совсем в другом файле. Три утверждения после потолка — против
пустого прохода, потому что потолку удовлетворяет и константа, и поле, к нему
подрезанное: поле не константа, ни один воксель не сидит ровно на потолке (плато
на нём есть подпись клампа, а ADR-041 запрещает кламп прямым текстом), и потолок
промахнут не на ширину округления.

Полоса, внутри которой блуждает начальное условие, ключом не является и
ратифицирована выводом: `excursion = min(typical, max − typical)/2`, целочисленным
делением к нулю (ADR-077). Из одного неравенства `2·excursion ≤ min(typical, H)`
следуют обе границы, обе точные — пол `2·(typical − excursion) ≥ typical` и
потолок `2·(typical + excursion) ≤ typical + max`, — и потому им место в тесте:
это вывод записи, а не константа, выбранная автором теста.
`the_initial_band_keeps_every_substance_strictly_above_zero` гоняет пол по
**всему** реестру фикстуры, а не по одному веществу: отвергнутая форма — доля
запаса до потолка — зелена на воде, у которой типичная концентрация есть 99%
предельной, и красна на кислороде, `SO4` и протоне, то есть ровно на
микрокомпонентах, ради которых заведён динамический диапазон ADR-039. Единичная
форма того же правила живёт в `the_band_is_bounded_by_both_headrooms`, вместе с
вырожденным случаем `typical = 0`.

Три имени про сторону слоя проверяют разное, и подменять одно другим нельзя.
`layer_sides_are_anticorrelated_across_the_boundary` — единственное, что ловит
сторону, разрешённую не на том индексе вещества: оксиклин формируется в другом
месте, оба ledger'а сходятся, и все прочие тесты остаются зелёными. Он же
единственный, кто держит **ориентацию**. Всё, что меряется через границу,
относительно — разбиение читается по собственному отклонению H₂S, — поэтому
глобальная инверсия сторон переименовывает группы вместе с полем и не краснеет
нигде, оставляя мир умолчания стратифицированным вверх ногами. Держат
ориентацию пол и крышка домена, две плоскости, которых рельеф не достаёт: там
слоевой член имеет знак, назначенный объявлением, а `|noise| ≤ UNIT` не даёт
бленду перевести вещество через типичное количество.

`a_substance_declared_uniform_has_no_layer_step` меряет и **ступень** через
границу, и **амплитуду**, и одной ступени мало. Отвергнутая форма
`blended = noise/2` ступени тоже не имеет, а разброс сужает ровно так же, как
снятие слоевого члена, — то есть проходит обе проверки, и ту, что смотрит на
ступень, и ту, что сравнивает ширины. Отличает её отношение двух ветвей на
плоскости пола, где слоевой член присутствует и постоянен: принятая форма даёт
два к одному, отвергнутая — один к одному. Числовых границ заливки в этом тесте
нет и быть не должно: ADR-077 отказывается их печатать, потому что они зависят
от усечения к нулю в `amount_at`, и напечатанное число стало бы благословением
сегодняшнего кода навсегда — а отношение двух прогонов одного генератора числом
фикстуры не является. `the_layer_side_does_not_change_with_the_seed`
отделяет принятую форму от отвергнутого знака слоя по `run_key` — и он
единственный, кто это делает, потому что отвергнутая форма делает
`different_seed_gives_a_different_initial_state` только зеленее.

`the_canonical_form_names_the_layer_side_of_every_substance` — прецедент ADR-065
одной секцией в сторону: умолчание печатается, а не подразумевается, иначе
идентичность прогона несёт утверждение, которого нет ни в одном файле.

**Расхождение с замороженной SPEC §12.4, три места** (ADR-032: спека не
правится). Первое: §12.4 называет три слоя — осадок, вода, воздух, — а
перечисление знает две стороны и «однородно»; воздуха в мире нет ни полем, ни
границей, и слой, которого не считает ни один процесс, был бы значением без
адресата. Второе: §12.4 обещает «всё параметризуемо», а секция параметризует
одну величину и языка генераторов не заводит — шум Уорли, поровая структура и
жерла остаются за её пределами. Третье: §12.4 требует симплекс-шума, а
`worldgen/` реализует решётчатый value noise со smoothstep-затуханием. От шума
здесь требуются **свойства** — `the_lattice_wraps_without_a_seam_in_x_and_y` и
`the_initial_field_has_structure_larger_than_a_voxel`, — а не имя алгоритма;
второе из этих имён гоняется обоими путями, слоевым и однородным, потому что
ADR-077 отверг ключ спектра измерением ровно этой статистики на обоих.

### Наблюдаемое поведение

Здесь «видна конвекция» превращается в число.

```
heated_bottom_produces_net_vertical_transport
density_stratification_persists_without_forcing
oxidation_front_forms_at_predicted_depth
prescribed_velocity_is_divergence_free_bit_for_bit
horizontally_uniform_temperature_produces_no_velocity
substance_with_zero_settling_radius_does_not_move_vertically
heat_and_matter_travel_the_same_distance_under_one_velocity_field
```

Это не проверка красоты, а проверка того, что механизм вообще включился.
Порог берётся грубым: конвекция либо переносит вещество вверх на порядок
сильнее диффузии, либо не работает. Точные значения — предмет калибровки, а не
приёмки.

Два первых имени с приходом поля скоростей (ADR-069) переписаны операционно, и
без оговорок они непроверяемы. `heated_bottom_produces_net_vertical_transport`
требует **локализованного** донного источника (SPEC §7, `VENT_BURST`) и `l_c` не
меньше четверти домена: при `l_c = 400 мкм` ячейки замкнуты, усиление идёт как
`Pe^{1/2}` и составляет 1.8× диффузии — порог «на порядок» не берётся не потому,
что механизм не работает. `density_stratification_persists_without_forcing`
ставится с `stir_fraction = 0`, устойчивой стратификацией и **ненулевым**
начальным горизонтальным возмущением `T`. Устойчивая стратификация на нулевом
тике берётся из умолчания `[initial.layer] = "sediment"` (ADR-077): каждое
вещество обогащено в осадке, и это единственный источник вертикального градиента
в корпусе — сценарий, объявивший все вещества `uniform`, лишает это имя
начального условия. Провалиться он может: возмущение
рассасывается кондукцией за `H²/α = 1170 с`, а опрокинуть столб течение успевает
за 768 с. Прежняя форма — «`stir_fraction = 0` и горизонтально однородная `T`» —
была нефальсифицируема: её прошло бы ядро, возвращающее нули.

`prescribed_velocity_is_divergence_free_bit_for_bit` обязан гоняться в обоих
режимах `Q`. Свойство держится не приближённо: хранится потенциал, `u` берётся
узкими разностями хранимых значений, и в дискретном `∇·(∇×A)` каждая компонента
входит дважды с противоположными знаками — сокращение происходит на одном и том
же округлённом числе. Глобальный множитель поверх `u` этим свойством не обладал
бы, и ровно поэтому рантайм-нормировки в решении нет.

`heat_and_matter_travel_the_same_distance_under_one_velocity_field` — из
ADR-087, и это операционная форма того самого отказа, ради которого
`process/tick.rs` две волны держал отказ вместо полудиспатча. Одно предписанное
поле, помеченное количество на мелкой сетке и помеченная аномалия энтальпии на
грубой; за `N` тиков шагов `b` и `c` центроиды обязаны сместиться на одно и то же
с точностью до одной грубой ячейки. Проваливается он на нулевом смещении тепла
при незаполненном `enthalpy_courant`, на четырёхкратном при среднем чисел Куранта
и на шестидесятичетырёхкратном при голой сумме — и все три при точно сходящихся
обеих невязках, потому что перенос сохраняет при любом числе Куранта.

`substance_with_zero_settling_radius_does_not_move_vertically` — из ADR-067:
`r = 0` не магическое значение, `k = 2·0²/9` есть точный ноль, и хост просто не
заводит процесс для вещества с нулевой `w`.

### Наблюдение

Соседний заголовок называется почти так же и стоит про другое. «Наблюдаемое
поведение» — про физику, которую мир обязан показать. Здесь — про аппарат,
которым это записывается: поток метрик ADR-037. Аппарат нуждается в собственной
приёмке ровно потому, что его отказы не роняют ни одного теста выше: мир
продолжает считаться правильно, а прогон перестаёт быть интерпретируемым.

```
the_first_record_is_the_run_header
the_residual_is_written_even_when_it_is_zero
a_truncated_tail_does_not_lose_the_rest
```

`the_first_record_is_the_run_header` — две половины, и вторая может проваливаться
годами. Первая: заголовок существует **до** первого тика. Писатель, выдающий его
лениво на первом тике, оставляет файл без заголовка ровно у того прогона, который
умер до тика один, — то есть у того, который и будут читать. Вторая: перечень
колонок не врёт. Заголовок, перечисляющий колонки, которых нет среди ключей
тиковой записи, — валидный NDJSON, лгущий о себе, и больше в корпусе этого не
видит никто; поэтому множества ключей сравниваются в обе стороны.

`the_residual_is_written_even_when_it_is_zero` — несущее требование ADR-037:
записанный ноль есть доказательство, что проверка выполнялась, а отсутствие
строки неотличимо от отключённой проверки. Тесту скармливается замкнутый домен
S0, где обе невязки точно нулевые, потому что это ровно тот тик, под которым
всякая удаляющая колонку оптимизация выглядит безобидно: `if r != 0`, список
пропуска нулей, «компактный» режим — каждая оставляет валидный JSON и не роняет
ничего, так как `assert_closed` живёт в другом месте и только в debug. Пройти
тест печатью константы нельзя: второй тик несёт ненулевые невязки. Колонок
невязки на одну больше числа веществ — ADR-028 делает инвариант двойным, и по
энергии он не выводится из вещественного.

`a_truncated_tail_does_not_lose_the_rest` — первое из трёх свойств, ради которых
ADR-037 выбрал NDJSON. Свойство относится к файлу целиком, поэтому проверяется на
каждом байтовом смещении, где прогон может умереть, и на тех входах, которые
единственно способны его сломать: `substance.id` — свободная строка TOML,
`Registry::new` стережёт дубликаты и число тридцать один и ничего про символы,
так что идентификатор с переводом строки достижим из сценария, а не гипотетичен.
Неэкранированный, он разрезает одну запись на две, и обе выглядят как NDJSON для
построчного читателя — с этого места свойство не держится, и не говорит об этом
никто. Оборванный хвост при этом сообщается отдельно, а не выдаётся как запись:
читатель, вернувший его записью, разобрал бы усечённый объект, и попавшие в него
числа выглядели бы полными.

`WORLD_FORMAT_VERSION` этими тремя не двигается и двигаться не должен (ADR-020):
поток метрик не меняет динамику, а перечень колонок версионируется отдельно —
удаление колонки обесценивает все прошлые прогоны, добавление нет. CI проверяет
только что версия сдвинулась, когда тронут стерегомый путь, и никогда — что она
устояла, когда не тронут ни один.

Критериев второго потока — снапшота для рестарта — здесь пока нет, и это не
упущение. Запись и чтение существуют и покрыты юнит-тестами, но три вещи, от
которых зависит формат, не решены и решаются не тестом: режим `Q` в файле, момент
сдвига версии формата и вынос записи из основного потока. Они помечены
`TODO(snapshot-layout)` и `TODO(snapshot-offthread)` в `observe/snapshot.rs`.
Третий вопрос — «нужно ли хранить производные поля вообще» — решён ADR-086, и
имена встают сюда:

```
poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world
the_denominator_is_fresh_for_step_b_and_for_step_h
a_restart_continues_the_run_bit_for_bit_from_the_tick_it_was_taken_at
the_snapshot_holds_no_buffer_of_class_q
every_buffer_the_scratch_owns_is_absent_from_the_snapshot
every_n_ticks_on_the_light_process_is_rejected
the_fold_credits_no_solar_energy_when_the_light_step_does_not_run
load_reports_the_per_voxel_footprint_of_the_world_and_the_scratch
```

Первое — операционная форма самого критерия владения: залить `Scratch` мусором
перед тиком, и мир после тика обязан совпасть побитово; буфер, классифицированный
неверно, падает здесь и больше нигде. Второе названо так, а не
`the_denominator_is_recomputed_before_each_of_its_two_readers`, чтобы его нельзя
было спутать с юнит-тестом `the_denominator_is_recomputed_and_not_cached`
(`kernels/temperature.rs`): тот утверждает, что кеша нет, этот — что свежесть
есть у обоих читателей — и **половина этой свежести сегодня непроверяема**:
головной пересчёт не виден ниоткуда, потому что оба прохода пишут одни и те же
два буфера, а единственный потребитель между ними — шаг `b` — не
диспетчеризуется. Тест проверяет хвостовой проход поведением, а головной —
только тем, что таблица `TEMPERATURE_SLOTS`, которую читает цикл тика, называет
оба слота (юнит-тест `the_temperature_has_a_slot_before_step_b_and_before_step_h`).
Четвёртое — страж критерия со стороны формата: файл
несёт ровно заголовок, оба буфера каждой полосы, оба буфера энтальпии и блок
счётчиков, и длина считается из мира, а не из литерала. Пятое названо от того,
чем тест является, — страж классификации, обходящий владельцев: он трогает
каждый из двенадцати буферов `Scratch` по одному и требует, чтобы побитовый
обход их видел, а файл — нет. Вторая половина каждого круга — «файл не
шевельнулся» — сегодня есть пересказ сигнатуры `snapshot_write`, которая
`Scratch` не принимает вовсе, и тест говорит это про себя вслух: он вооружается в
день, когда сигнатура его получит.

Третье живое, а не `#[ignore]`, и в этом вся разница с вариантом, который
ADR-086 отверг: диффузия и адвекция диспетчеризуются сегодня, значит снять
снапшот в середине прогона, влить и досчитать можно уже сейчас — и побитовое
совпадение с непрерывным прогоном есть то самое обещание ADR-016, которое до
этой записи было доказуемо невыполнимым. Седьмое — про `FoldParams::i_surface`:
верхний член поглощения — единственный, которого нет в поле, поэтому при
выключенном шаге `a` он обязан быть точным нулём, иначе свёртка создала бы
верхний слой энергии из ничего и зачислила его в `SOLAR_IN` при **точно**
нулевой невязке.

Шестое в этом перечне стоит одно, а в перечень отказов S0 выше их прибавилось
**два**: следствие ADR-086 гласило «список растёт на одно имя, второе ADR-074
туда уже внёс», и это неверно — `every_n_ticks_on_the_velocity_field_is_rejected`
ADR-074 назвал, но в перечень не внёс, и его там не было. Оба стоят выше, в
порядке таблицы §10 `CONFIG_SCHEMA.md`.

Восьмое — то, что ADR-086 даёт **вместо** потолка памяти в байтах: потолок
выводить не из чего (`S_MAX`, `R_MAX` и `N_MAX` уже ограничивают каждый
сомножитель, а ключом сценария он был бы декорацией), поэтому загрузчик печатает
байты на воксель у `World`, у `Scratch` и сумму — до аллокации `Scratch`, чтобы
прогон на `R = 64` видел свой гигабайт прежде, чем попросит его у машины.
Проекция и аллокация — два вычисления одного числа, поэтому тест держит их друг
против друга, а измеряет через `ScratchBuffers`: тринадцатый буфер двигает
измерение и роняет тест, пока проекция не двинулась с ним. Прецедент формы —
`load_reports_the_restoration_traffic_per_tick` (ADR-057).

### Вьюер

Третий читатель мира после потока метрик и снапшота, и единственный, у которого
читатель — человек. Отсюда особая форма его отказов: неверная картинка выглядит
правдоподобно. Смещённый на байт заголовок объёма рисует облако, транспонированный
объём рисует облако, объём с write-стороны буфера рисует облако, а нули нужной
длины рисуют мёртвый мир — и ни одно из четырёх не роняет ничего выше по стеку,
потому что количества при этом не менялись и ни одна проверка сохранения этого не
видит. Транспорт — четыре HTTP-маршрута (ADR-070), квантование — против
объявленного `max_conc` (ADR-072), невязка — трёхзначная (ADR-071).

```
the_server_serves_a_volume_of_the_declared_shape
a_volume_byte_is_fixed_by_the_declared_maximum_not_by_the_frame
the_volume_is_indexed_the_way_the_grid_is
the_volume_reads_the_front_buffer_and_never_the_write_side
an_unknown_field_is_a_404_and_not_an_empty_volume
the_profile_runs_from_the_floor_upwards
the_state_route_reports_both_residuals
the_residual_is_unreported_before_the_first_tick
the_published_residual_is_computed_and_not_written
the_reported_matter_residual_does_not_cancel_between_substances
a_dead_simulation_stops_claiming_a_closed_ledger
control_pause_actually_stops_the_tick_counter
step_advances_exactly_one_tick_while_paused
the_reported_rate_is_measured_and_not_the_requested_one
an_unknown_control_action_is_refused_rather_than_ignored
a_seed_past_two_to_the_53_is_not_truncated_by_the_page
a_substance_id_that_would_break_the_json_is_escaped
the_four_routes_are_the_ones_the_viewer_asks_for
the_viewer_is_served_whole
an_unknown_route_is_a_404_and_not_the_viewer
the_wrong_method_is_a_405_that_says_which_one_was_wanted
```

`the_server_serves_a_volume_of_the_declared_shape` проверяет заголовок побайтно
именно потому, что страница его не проверяет вовсе: она читает `DataView(buf, 0,
24)` и `Uint8Array(buf, 24)` и разъехавшуюся границу не заметит. В том же тесте —
`scale`: число, от которого не зависит ни один пиксель (шейдер берёт нормированную
выборку), а значит ошибка в нём невидима глазами и видна только тому, кто читает
объём программой.

`a_volume_byte_is_fixed_by_the_declared_maximum_not_by_the_frame` — единственный,
кто ловит нормировку на максимум кадра, и он же объясняет, почему её надо ловить:
байт одного и того же вокселя обязан не измениться, когда изменился только
экстремум поля. Количества при этом те же, поэтому ни один тест на сохранение
здесь не срабатывает.

`the_volume_reads_the_front_buffer_and_never_the_write_side` ловит два промаха
разом — `lane == s` (ADR-056) и чтение write-стороны (ADR-057), — и оба
недоказуемы на произвольной фикстуре. Поэтому тест сначала утверждает, что
фикстура **способна** их различить: что хотя бы одно вещество лежит на дорожке
другого номера и что хотя бы у одной дорожки две стороны буфера различны. Без этих
двух утверждений он зелен на мире, где промах ненаблюдаем.

`the_state_route_reports_both_residuals` и два соседних имени делят между собой
трёхзначность ADR-071: число на завершённом тике, `null` до первого тика,
`null` после смерти потока симуляции. Слить два последних состояния в ноль — ровно
то, что ADR-037 называет неотличимым от отключённой проверки, только под зелёной
плашкой.

`the_published_residual_is_computed_and_not_written` — тот же несущий вопрос, но
про сам маршрут, и здесь у него нет естественного входа. Все процессы S0 сохраняют
каждое вещество точно, а `Ledger::assert_closed` срабатывает **внутри**
`Tick::advance` в debug-сборке, — значит ни один тик, доступный тесту, не может
дойти до публикации с дырой, и константа `matter: 0` осталась бы зелёной во всех
маршрутных тестах выше. Поэтому дыра делается тестом прямо в аккумуляторах, из
которых публикация читает, и делается несимметричной: `+7` у одного вещества и
`−3` у другого дают `+7` как написано и `−7` при переставленных местами
аккумуляторах. Одна строка порядка при этом не покрыта и покрыта быть не может —
`advance_one` говорит, какая именно и чем это кончится.

`control_pause_actually_stops_the_tick_counter` состоит из двух половин, и первая
существует ради второй: без утверждения, что счётчик **двигался**, тест зелен на
симуляторе, который не запускался вовсе. Пауза проверяется не флагом, а двумя
измерениями счётчика через промежуток: флаг, который читает только сам себя,
ставится и в цикле, который продолжает считать.

`a_seed_past_two_to_the_53_is_not_truncated_by_the_page` и
`a_substance_id_that_would_break_the_json_is_escaped` — два способа для страницы
показать чужой прогон и не сказать об этом. `JSON.parse` режет за 2^53, поэтому
`seed` уезжает строкой; `substance.id` — свободная строка TOML, и кавычка в ней
разрывает объект, страница падает в свой `catch` и печатает «Lost the simulator»,
то есть винит связь.

Живут эти тесты в `crates/liminis/src/{serve,http,main}.rs` — в приватных `mod
tests` внутри бинарного крейта, а не в `tests/`, потому что маршруты и роутер не
экспортируются: снаружи бинаря нет ни `Sim`, ни `Response`. Это и есть причина,
по которой перечень имён нужен здесь: иначе критерий не находится ничем, кроме
чтения исходника.

Одного критерия в этом списке нет: **ни один сценарий в `configs/` не строит мир**
(ADR-070 называет эту цену прямо). Мир, на котором гоняются все имена выше, —
`#[cfg(test)]`-фикстура внутри `serve.rs`, и копировать её в `configs/` нельзя:
`c_p`, `enthalpy_formation` и `partial_molar_volume` в ней помечены как заглушки,
потому что этих величин не объявляет в корпусе никто (`CONFIG_SCHEMA.md` §13,
пункт 23). Имя `every_shipped_scenario_can_be_served` встанет сюда вместе с
решением, откуда берутся термохимические величины веществ, — вопрос A-18.

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

---

## Локальная культура конечных экотипов (ADR-091, ADR-092)

Это первая наблюдаемая жизнь экорежима, не приёмка клеточного морфогенеза
или существенного эволюционного перехода. Конечные полосы наследуемых
стратегий заменяют ещё не реализованные поля BT из замороженной SPEC;
расхождение и ограничения названы в ADR-091.

Исполняемые критерии в `crates/liminis-core/tests/acceptance_living_world.rs`:

```text
living_ecotypes_share_chemistry_and_have_a_real_resource_tradeoff
birth_mutations_create_neighbours_and_removing_them_prevents_new_types
living_culture_changes_heritable_shares_and_closes_both_ledgers_each_tick
living_culture_replays_identically_from_the_same_seed
living_catalysts_are_fresh_and_cannot_create_unseeded_life
living_world_10k_ticks_closes_integer_ledgers_and_stays_within_ceilings
```

Последний критерий запускается отдельно с `--release -- --ignored`:
полная сетка 24³, 10 000 тиков, оба целочисленных баланса проверяются каждый
тик и в release. Остальные входят в обычный набор тестов. Проба с единственным
родительским вариантом отделяет мутацию при рождении от простого присутствия
всех вариантов в начальном состоянии; стерильная проба исключает рост без
катализатора. Изменение долей считается отбором внутри конечного набора,
а не появлением ранее не заданного генома.

Команды, проверенный горизонт и измеренные доли записаны в
`docs/plans/2026-10-04-local-living-world.md`.

---

## Генетическая колония из одного основателя (ADR-093, ADR-094)

Следующая ступень остаётся популяционной моделью: четыре генотипа задаются
двумя локусами, а не таблицей отдельных клеток. Критерии не утверждают
морфогенез, индивидуальную родословную или неограниченную эволюцию.

Исполняемые причинные проверки в
`crates/liminis-core/tests/acceptance_genetic_colony.rs`:

```text
a_single_founder_creates_new_genotypes_only_through_single_locus_births
inherited_speed_and_affinity_reverse_fitness_between_poor_and_rich_food
producer_turnover_supplies_detritus_that_changes_consumer_growth
the_colony_expands_a_measured_biomass_contour_and_replays_exactly
genetic_colony_10k_ticks_closes_both_ledgers_and_retains_evolved_populations
```

Первая проверка оставляет G11 точно пустым после первого тика G00 и проверяет,
что без мутаций все три незаселённых генотипа остаются пустыми. Вторая меняет
пищевую среду и требует смены преимущества между аллелями. Третья выключает
оборот биомассы, чтобы доказать источник детрита, затем сравнивает одинаковые
популяции потребителя с произведённым детритом и без него. Четвёртая измеряет
радиус контура 0.0003 моль/м³: это 1% концентрации инокулюма, а не любой
ненулевой хвост диффузии. Последняя запускается отдельно в release на полной
сетке 24³ и явно проверяет оба целочисленных баланса каждый из 10 000 тиков.

Генератор проверяется отдельно в `tests/acceptance_inoculation.rs`:
объявленный ноль, геометрия физической сферы, правильная полоса при смешанных
разрядностях, отчёт после заполнения и отсутствие трёх генотипов в нулевом
тике поставляемого сценария. Канонический повторный разбор, конфликт ручной
и производной кинетики и покрытие генетических параметров хешем проверяются
рядом с компилятором в `config/genetics.rs`.

Проверенные команды и результаты записываются в
`docs/plans/2026-10-04-genetic-colony.md`.

---

## Долговечный локальный опыт (ADR-096, ADR-097)

Сохранение наблюдения не считается сохранением мира. Причинная проверка
полного restart находится в `tests/acceptance_genetic_colony.rs`:

```text
full_checkpoint_resume_replays_genetic_colony_and_channel_counters
full_checkpoint_capture_reports_the_shipped_colony_copy_cost
```

После 137 тиков snapshot загружается в свежие World/Scratch/Ledger, собранные
из сохранённой канонической конфигурации. Ещё 89 тиков должны дать побайтно
тот же полный snapshot, что и непрерывный прогон: оба буфера всех полей и
целые счётчики каналов. Проверяются seed 42 и максимальный u64; ledger
проверяется каждый тик, в том числе release. Вторая проверка измеряет объём
и длительность копии полной поставляемой сетки 24³ без нестабильного порога
времени в assert.

Хост дополнительно обязан проверить отказ на повреждённом/неполном checkpoint,
неверной identity и лишнем хвосте; атомарную публикацию поколения и writer ack;
единственного владельца run; отдельный namespace при reset; восстановление
observer metadata; parent-границу NDJSON при resume; реальный остановленный
и затем продолженный процесс. Невязка до следующего исполненного тика
неизвестна, а не равна придуманному нулю.

Проверенные команды, ошибки диска/ввода и браузерные результаты записываются в
`docs/plans/2026-10-04-durable-living-experiment.md`.

---

## Индивидуальные клетки в однородной камере (ADR-099, ADR-100, ADR-101)

Отдельная команда `cells` принимает самостоятельный chamber format 1.
Это ограниченный прототип с настоящими объектами клеток, не пространственный
S1′: растворённые вещества однородны, физические координаты, моторика,
адгезия, GRN и тела отсутствуют. Экранная раскладка не является биологическим
поведением. Изменяется один ограниченный наследуемый локус кинетики.

Причинные проверки в `tests/acceptance_cell_chamber.rs`:

```text
sterile_supplied_medium_does_not_create_cells
starvation_lyses_actual_cells_without_losing_matter_or_energy
zero_mutation_preserves_the_entire_inherited_genome
restored_future_matches_exact_cells_and_independent_cumulative_ledgers
```

Дополнительные проверки `micro::tests` независимо сводят точные переносы
вещества/энергии при росте, содержании, делении и лизисе; перестановка таблицы
клеток не меняет распределение субстрата и выдачу потомкам ID. Превышение
защитного предела объектов явно останавливает опыт и не публикует частичный
тик. Предел не считается физической ёмкостью среды и не подавляет деление.

Отдельный критерий поставляемого сценария:

```text
micro::config::tests::shipped_cell_chamber_sustains_real_turnover_for_10k_steps
```

Он запускается явно в release с `--ignored --nocapture`, требует настоящий
оборот клеток, выживших, отсутствие срабатывания лимита и оба точных баланса
на каждом из 10 000 шагов. Это не обещание бессрочной жизни или длительной
диверсификации: в проверенном опыте возникли аллели -1/0/+1, но в конце
остался только 0.

Хост проверяет атомарность core/observer, сохранность всех 64 бит seed,
полный будущий replay и наследуемую float-физиологию через настоящий JSON
checkpoint. При resume он заново сводит полный накопленный баланс, lifecycle
счётчики и `next_cell_id`, прежде чем открыть новую ветвь истории.
Хранилище отдельно проверяет checksum/размер, kind/identity, единственного
writer, ограниченное сохранение поколений, границу истории, оборванный
последний хвост и отказ на второй JSON value. Эквивалентные JSON числа и
пробельный хвост не считаются повреждением.

Текущий world 30 меняет eco competition arithmetic (ADR-102): eco28/29
отвергаются и требуют matching legacy executable. Узкое продолжение eco28
под world29 было историческим решением ADR-100, отменённым ADR-103.
Отдельные cells29 допускаются после полной проверки и сохраняют identity29
при продвижении и следующем checkpoint; новый cell опыт получает30.
Неизвестные версии и смешение eco/cells отвергаются; core codec не ослаблен.

Результаты release suite, независимого 10k-прогона, browser QA и двух
настоящих process restart записаны в
`docs/plans/2026-10-04-cell-chamber.md`. Невязка после восстановления остаётся
неизвестной до первого реально проверенного тика.

## Opt-in point transport клеточной камеры (ADR-108)

Отдельный chamber2 добавляет persisted center positions в reflecting box,
сохраняя well-mixed chemistry. Это узкое CPU/native-f64 исключение из
ADR-015/022 и frozen SPEC §12.2–12.3, частичная отмена ADR-099 для opt-in;
приёмка voxel S1′, collisions и cross-platform bit equality не заявляются.

Причинные и численные gates:

```text
legacy_canonical_bytes_hash_and_absent_spatial_declarations_are_preserved
spatial_requires_explicit_entry_points_and_preserves_biology_derivation
chamber_declarations_are_complete_and_do_not_hide_a_second_volume_source
transport_draw_anchors_full_ordered_le_tuple_and_strict_midpoint_domain
stokes_einstein_absolute_si_oracle_and_parameter_proportionalities
zero_mobility_preserves_coordinate_bits_at_walls_and_subnormal_exactly
preexisting_mass_sets_radius_and_daughters_inherit_one_parent_endpoint
unsupported_numeric_range_rejects_entire_tick_after_earlier_candidate_motion
free_fixed_coefficient_multilag_msd_matches_six_dt_without_wall_filtering
reflected_uniform_ensemble_preserves_box_distribution_at_small_and_multiple_wall_steps
represented_reflection_fixtures_include_walls_multiwrap_and_negative_roundoff
fma_preserves_cancellation_and_numeric_budget_refuses_unsupported_scales
eco_world30_actual_disk_resume_keeps_identity_and_exact_future
```

Абсолютный SI oracle вычислен отдельно от production formula. Gaussian
moments/tails и free MSD используют заранее заданный ensemble; `6Dt` относится
к increments при фиксированном D до стен. Arithmetic allowance ограничивает
represented endpoint/reflection; coefficient/libm/drift остаются отдельными
ограничениями. Actual disk tests проверяют cells identity/positions и eco30
advance→save→second resume без нового version tag.

Native release benchmark именует D0/Dpositive scenario, seed, hardware,
binary/config/source hashes, occupied observations, ticks, latency и Pause
ack. Он не является универсальным throughput обещанием. Настоящий protected
Playwright проверяет оба chamber formats последовательно, sandbox/CDP argv,
actual API positions и Step, held state/display30/60, Save acknowledgement, desktop/mobile
captions; inventory layout остаётся schematic. Final CI/head/review/evidence
фиксируются в `docs/checkpoints/2026-10-04-live-physical-transport.md`.
Resume и exact future проверяют отдельные actual disk tests; браузерный runner
не перезапускает host и не заявляет browser resume.

## LIVE-3a: physical2D projection без нового model solver (ADR-109)

Это dependent observer этап после LIVE2; final-head browser/CI gates обязательны.
`node --test scripts/cell-viewer.test.mjs` проверяет nonsquare dimensions/equal
scale во всехplanes, reflected right/up projection, µm/closedclippedslice/
zero-thicknessplane, exactdecimalID/BigIntsorting/coincidenthits, absentselection
без death inference и synchronous snapshot commit до независимогоhistory.
Эти Node checks не доказывают actualrendered browsergeometry.

`node scripts/check_physical_projection_browser.mjs` требует защищённый pinned
Chromium с actualreleasehost и clean expectedhead. Repositoryphysicalscenario
не изменяется: temporarynonsquarefixture сохраняет volumeproduct и имеет свой
byte/hash provenance. Gate независимо вычисляет ожидаемые markercenters из
actualAPI и наблюдённой canvasbox, сверяет actualpaint/pixels и pointer/keyboard
inspector, allplanes/equalunit/aspect/µm, clippedinclusive/zero-widthslice,
unknown/vanishedID и retainedoutside selection. ActualpausedSteps до настоящего
division ограничены; coincidence daughters и выбор каждого проверяются по
actualstate/event, без fakeAPI или разделения центров. Delayed actualhistory
проверяет согласованность новогоtick/readouts/canvas/inspector до его доставки;
никакое новоеmodelstate не фабрикуется. Pausedprojectioncontrols не создают
modelcontrol/ticks и не повышают polling; 30/60display остаётся отдельной целью.

Defaultchamber1 regression сохраняет inventoryschematic и старые controls;
opt-inchamber2 captions описывают centers/visualmarker/sharedchemistry.
Desktop/mobile/shortviewport реальныеPNG, trace и report публикуются отдельно,
после cleanup verdict; code/geometry review независимо от authors. Срез только
фильтрует центры в изображении; он не меняет volumes, biology, numericbudget
или savedpositions. Everytick publicrecording является другим отдельным этапом.

## REC-2: lossless every-tick архивные записи (ADR-110)

Каждый реальный tick0…H для H=10000/100000/1000000; один shared trajectory,
старые JSON/URL/SHA неизменны. Source identity world30/chamber1 и clean archival
producer закрепляются отдельно от нынешнего main/world31. Полный native
последовательный decode обязан восстановить H+1states, точно сравнить все
общие архивные frames/known genomes, подтвердить оба producer ledger0 и cap.
Проверенный externally pinned coordinator receipt допускает publisher reuse
без повторной симуляции/decode; все gzip size/SHA/CRC/inflated SHA всё равно
перечитываются. Path-only sharding и полная metadata входят в3000000000bytes.

Source gates: `python3 scripts/dense-recording-tests.py` и
`node --test site/*.test.mjs scripts/cell-viewer.test.mjs`. Закреплённый catalog
строится только из complete closed inventory, хеширует каждый bounded файл,
проверяет shared prefixes/provenance и immutable archive. Это не browser PASS.

`node scripts/check_dense_recording_browser.mjs` выполняется только в существующем
protected pinned Chromium job с chromiumSandbox/CDP argv и expected HEAD/tree.
Обязательны настоящие saved Python reference frames/negative transport controls,
native fetch abort и stale intents, delayed original chunk255→256, actual
endpoints всех трёх horizons, requested/validated tick separation, held pixels,
измеренная1× model/wall скорость, display30/60, bounded caches, mobile/desktop
PNG/trace и final cleanup/error verdict. Никакие ответы/states не фабрикуются.
BFCache wiring unit check именуется synthetic; native BFCache PASS не заявляется.

Cancellation proof связывает native AbortSignal/reason/order с точным CDP
requestId и завершением actual body; deadline/late cleanup после ошибки и
ambiguous identities не проходят. Отдельная signal-only фаза сохраняет native
Promise rejection semantics без observer handlers/Reader wrappers: real
pre-abort и held original mid-body abort обязаны не создавать orphan errors.
Один synthetic native orphan positive control допускается только по exact
reason identity, сохраняется raw и явно обозначает чувствительность error
channel, не recording evidence. Unknown/repeated events запрещены. Итоговый
request/error aggregate повторяется после bounded cleanup, sticky FAIL.

Существующий public browser gate отдельно проверяет deployed catalog/index/
manifest/module/gzip SHA, реальные browser response bytes и dense Next0→1,
1×/held state. Известная фиксированная Cloudflare analytics insertion проверяется
по отдельному source pin и блокируется; analytics execution PASS не заявляется.
Документированные hosting limits не заменяют actual upload/deployment verdict.
Следующее расширение existing main-only public gate отдельно закрепляет Node
HTTPS byte pins metadata и final chunk каждого dense horizon. Это bounded
transport проверка, не public100k/1M endpoint rendering/full dataset download;
browser smoke по-прежнему проверяет default10k.
Evidence и pending boundaries:
`docs/checkpoints/2026-10-04-dense-recorded-observer.md`.

## LIVE-3b: ортографический Three.js observer реальных центров (ADR-111)

Chamber2 получает opt-in3D при прежнем2D default; chamber1 остаётся schematic.
Model/core/config/version, сохранённые координаты и архивные записи неизменны.
`node --test scripts/cell-viewer.test.mjs scripts/cell-viewer-3d.test.mjs`
проверяет common xyz scale/finite geometry, exact ID/candidate sorting,
camera revision и gesture/lifecycle boundaries. Focused host tests проверяют
literal local module routes, byte pins/import closure и JavaScript MIME.
Эти source checks не являются browser PASS.

`node scripts/check_physical_3d_browser.mjs` работает только в существующем
protected official pinned Chromium с clean expected HEAD/tree и CDP argv guard.
Требуется actual WebGL2/backend, независимый API→canvas-pixels oracle для cube
и temporary nonsquare fixture с тем же volume product, handedness/z/common
SI scale/aspect/µm и actual camera pointer rotation. Production hit-map или
Three.js projection helpers не являются independent expected geometry.

Paused rotate/zoom/pan/reset/slice/layers/opacity и draw30/60 не меняют
actual tick/positions/dt/pacing и не создают modelcontrols/extra polling.
Delayed original history после настоящего Step проверяет атомарные
geometry/readouts/inspector. Actual bounded Steps дают настоящих coincident
daughters без fabricated API; short-click/cancel/drag-return и keyboard
проверяют projected candidates по distance затем exact BigInt ID. Hidden,
outside slice/view и absent IDs имеют разные честные статусы.

Actual compositor pixels подтверждают opacity/layers и отсутствие hidden hit
targets, counts/data остаются прежними. Полная vendored import closure и SHA,
отсутствие внешних dependency requests, один context/current-population
resources, explicit current2D fallback при unavailable/loss входят в gate.
Synthetic GPU-loss control подписан отдельно; software GPU не hardware FPS.
Desktop/short/390/320 PNG, trace/report и итоговые cleanup/errors публикуются
на exact candidate, source/artifacts принимает независимый reviewer.
Official Playwright1.61.1 baseline уже содержит штатный
`--enable-unsafe-swiftshader`/ANGLE SwiftShader switches: они фиксируются в
observed argv/backend как qualification прежней конфигурации. Новый запуск
строго `headless:true, chromiumSandbox:true, args:['--enable-automation']`,
без добавленных GPU flags; прежние sandbox запреты и запреты отключения
web security/GPU blocklist сохраняются. Software geometry acceptance не
означает hardwareFPS или аттестацию изоляции ядра.
Существующие chamber1/livephysical/2D projection gates сохраняются.
Scope/pending: `docs/checkpoints/2026-10-04-live-physical-observer-3d.md`.


## LIVE-3b: точная paused compositor identity без привязки к PNG encoding

Historical main5bea/run37241483046 остаётся FAIL: encoded PNG SHA отличается,
а original per-canvas captures/post-FPS audits отсутствуют. Причина неизвестна;
новая проверка не устанавливает, что прежние pixels были одинаковыми.

Fresh protected13GL gate сохраняет baseline/30/60 original PNG, dimensions,
encoded и decodedRGBA SHA, pre/post API/camera/viewport/layers до assertions.
Dimensions и каждый RGBA byte всей прежней compositor области должны совпасть;
opaqueRGB получает alpha255. Любое отличие pixel/alpha/dimensions — FAIL
с changedcount/bounds; masking/cropping/tolerance/retries не разрешены.
Actualstate/positions/tick/dt/pacing/pollcount сохраняются independently.
Synthetic5filters/encoding/RGBalpha/dimensions/error controls проверяют actual
QA decoder/comparator, не browserPASS. Production3D/core/site/recordings
не меняются. Separatefreshmain head/independentreview/fullCI/actual13GL и
прежниеREC/dense/physical regressions обязательны; old79/c02PASS не переносится.
LABcandidate/publicLAB/models остаются отдельным HOLD.

## REC observer: HUD и canvas в раздельных строках

Recorded well-mixed observer отделяет normal-flow header/title/stats и
frame-stamp footer от canvas. Glyphs остаются schematic inventory, не
физическими координатами. CSS layout не меняет source observations,
decoded cells/genomes/counters, clock/dt/×N, display targets или decoder.
Данные/old URLs/SHA не пересобираются, observer.js остаётся неизменным.

Существующие protected `site/playback.browser.mjs` и
`scripts/check_dense_recording_browser.mjs` обязаны сохранять DOM rectangles:
header/title/stats/footer и полный текст внутри своих bounds без overflow,
canvas не пересекается с HUD; usable canvas после прежнего padding36px
положителен по обеим осям. Проверяются desktop1440×900, short1440×720,
mobile390×844 и320×844 с настоящими PNG/trace. Canvas нельзя скрыть, закрыть
overlay или обрезать ради прохождения; счётчики не заменяются ellipsis.

Dense10k tick992 должен показывать все216 actual cells и counters того же
независимо decoded frame. После resize проверяются настоящие keyboard и
pointer selection с exact ID/parent/genome/mass_units/energy_units inspector;
pointer coordinates относятся к actual canvas rect/pixels. Initial/final
состояния и sparse archive path сохраняют свои source identity и readouts.
Размер canvas после layout change может измениться; при фиксированном
viewport paused30/60 pixel/state/byte oracles остаются строгими, без
mask/crop/tolerance/retry. Sandbox/observedargv/network/body/cancellation/
unhandled/cleanup/time guards и mandatory full CI сохраняются.

Нужны independent exact-source review, новые actual browser artifacts на
accepted HEAD/tree и после merge отдельный public deployment/playback gate.
HTML/CSS served byte pins выводятся из нового source, старые PASS не
переносятся. Native Node syntax/source checks не являются browserPASS.
LAB21/publicLAB/LAB4/models сохраняют Astra HOLD; legacy mobile sticky
transport/sidebar overlap остаётся отдельным отложенным finding.
Scope/pending: `docs/checkpoints/2026-10-05-recorded-hud.md`.
